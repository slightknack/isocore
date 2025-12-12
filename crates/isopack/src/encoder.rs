use std::mem;

use super::types::Result;
use super::types::Error;
use super::types::Tag;
use super::macros::encode_wrapper_method;
use super::macros::for_each_multibyte_scalar;
use super::macros::encode_wrapper_api;
use super::macros::encode_record_multibyte;
use super::macros::encode_array_multibyte;
use super::macros::encode_root_multibyte;

/// A growable buffer that encodes data into the NeoPack format.
pub struct Encoder {
    pub buf: Vec<u8>,
    last_flush: usize,
    open_scopes: usize,
}

impl Encoder {
    pub fn new() -> Self {
        Self { 
            buf: Vec::new(),
            last_flush: 0,
            open_scopes: 0,
        }
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self { 
            buf: Vec::with_capacity(cap),
            last_flush: 0,
            open_scopes: 0,
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.buf
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    /// Flush all bytes written since the last flush
    /// Returns a slice of the newly flushed bytes
    /// Can only flush when all containers are closed
    pub fn flush(&mut self) -> Result<&[u8]> {
        if self.open_scopes > 0 {
            return Err(Error::ScopeOpen);
        }
        let slice = &self.buf[self.last_flush..];
        self.last_flush = self.buf.len();
        Ok(slice)
    }

    /// Take ownership of all flushed bytes and compact the buffer
    /// This removes flushed bytes from the buffer to free memory
    pub fn take_flushed(&mut self) -> Vec<u8> {
        let taken = self.buf[..self.last_flush].to_vec();
        self.buf.drain(..self.last_flush);
        self.last_flush = 0;
        taken
    }

    /// Create an encoder from existing bytes
    /// Validates that bytes contain complete neopack messages
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        use crate::Decoder;
        
        // Validate by parsing
        let mut decoder = Decoder::new(&bytes);
        while decoder.remaining() > 0 {
            decoder.skip_value()?;
        }
        
        Ok(Self {
            buf: bytes,
            last_flush: 0,
            open_scopes: 0,
        })
    }

    #[inline(always)]
    fn write_tag(&mut self, tag: Tag) {
        self.buf.push(tag as u8);
    }

    #[inline(always)]
    fn write_u32_raw(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn write_blob(&mut self, tag: Tag, data: &[u8]) -> Result<()> {
        if data.len() > u32::MAX as usize {
            return Err(Error::BlobTooLarge(data.len()));
        }
        self.write_tag(tag);
        self.write_u32_raw(data.len() as u32);
        self.buf.extend_from_slice(data);
        Ok(())
    }

    #[inline]
    pub fn bool(&mut self, v: bool) -> Result<&mut Self> {
        if self.buf.len() >= u32::MAX as usize {
            return Err(Error::ContainerFull);
        }
        self.write_tag(Tag::Bool);
        self.buf.push(v as u8);
        Ok(self)
    }

    #[inline]
    pub fn u8(&mut self, v: u8) -> Result<&mut Self> {
        if self.buf.len() >= u32::MAX as usize {
            return Err(Error::ContainerFull);
        }
        self.write_tag(Tag::U8);
        self.buf.push(v);
        Ok(self)
    }

    #[inline]
    pub fn i8(&mut self, v: i8) -> Result<&mut Self> {
        if self.buf.len() >= u32::MAX as usize {
            return Err(Error::ContainerFull);
        }
        self.write_tag(Tag::S8);
        self.buf.push(v as u8);
        Ok(self)
    }

    for_each_multibyte_scalar!(encode_root_multibyte, ());

    pub fn str(&mut self, v: &str) -> Result<&mut Self> {
        self.write_blob(Tag::String, v.as_bytes())?;
        Ok(self)
    }

    pub fn bytes(&mut self, v: &[u8]) -> Result<&mut Self> {
        self.write_blob(Tag::Bytes, v)?;
        Ok(self)
    }

    pub fn list(&mut self) -> Result<ListEncoder<'_>> {
        if self.buf.len() >= u32::MAX as usize {
            return Err(Error::ContainerFull);
        }
        self.write_tag(Tag::List);
        Ok(ListEncoder::new(self))
    }

    pub fn map(&mut self) -> Result<MapEncoder<'_>> {
        if self.buf.len() >= u32::MAX as usize {
            return Err(Error::ContainerFull);
        }
        self.write_tag(Tag::Map);
        Ok(MapEncoder::new(self))
    }

    pub fn array(&mut self, item_tag: Tag, stride: usize) -> Result<ArrayEncoder<'_>> {
        assert!(stride > 0 && stride <= u32::MAX as usize, "invalid stride: {}", stride);
        if self.buf.len() >= u32::MAX as usize {
            return Err(Error::ContainerFull);
        }

        self.write_tag(Tag::Array);

        let len_offset = self.buf.len();
        self.write_u32_raw(0); // Placeholder for ByteLen

        self.buf.push(item_tag as u8);
        self.write_u32_raw(stride as u32);

        let body_start = len_offset + 4;

        Ok(ArrayEncoder {
            scope: PatchScope::manual(self, len_offset, body_start),
            stride,
        })
    }

    pub fn record_raw(&mut self, v: &[u8]) -> Result<&mut Self> {
        self.write_blob(Tag::Struct, v)?;
        Ok(self)
    }

    /// Starts a standard Record (opaque struct with a Tag and Length header).
    pub fn record(&mut self) -> Result<RecordEncoder<'_>> {
        if self.buf.len() >= u32::MAX as usize {
            return Err(Error::ContainerFull);
        }
        self.write_tag(Tag::Struct);
        Ok(RecordEncoder {
            scope: PatchScope::new(self)
        })
    }
}

struct PatchScope<'a> {
    parent: &'a mut Encoder,
    len_offset: usize,
    body_start_offset: usize,
}

impl<'a> PatchScope<'a> {
    fn new(parent: &'a mut Encoder) -> Self {
        let len_offset = parent.buf.len();
        parent.buf.extend_from_slice(&[0; 4]);
        let body_start_offset = parent.buf.len();
        parent.open_scopes += 1;
        Self { parent, len_offset, body_start_offset }
    }

    fn manual(parent: &'a mut Encoder, len_offset: usize, body_start_offset: usize) -> Self {
        parent.open_scopes += 1;
        Self { parent, len_offset, body_start_offset }
    }

    fn check_size(&self) -> Result<()> {
        let current_len = self.parent.buf.len();
        let body_len = current_len.saturating_sub(self.body_start_offset);
        if body_len > u32::MAX as usize {
            return Err(Error::ContainerFull);
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        self.check_size()?;
        let current_len = self.parent.buf.len();
        let body_len = current_len.saturating_sub(self.body_start_offset);
        let len_bytes = (body_len as u32).to_le_bytes();
        let dest = &mut self.parent.buf[self.len_offset..self.len_offset + 4];
        dest.copy_from_slice(&len_bytes);
        Ok(())
    }

    fn finish(mut self) -> Result<&'a mut Encoder> {
        self.flush()?;
        self.parent.open_scopes -= 1;
        let parent_ptr = self.parent as *mut Encoder;
        mem::forget(self);
        Ok(unsafe { &mut *parent_ptr })
    }
}

impl<'a> Drop for PatchScope<'a> {
    fn drop(&mut self) {
        let _ = self.flush();
        self.parent.open_scopes -= 1;
    }
}

pub struct ListEncoder<'a> {
    scope: PatchScope<'a>,
}

impl<'a> ListEncoder<'a> {
    fn new(parent: &'a mut Encoder) -> Self {
        Self { scope: PatchScope::new(parent) }
    }

    encode_wrapper_api!([&mut self], &mut Self, '_;
        parent: self.scope.parent;
        pre: {};
        post: self
    );

    pub fn finish(self) -> Result<&'a mut Encoder> {
        self.scope.finish()
    }
}

pub struct MapEncoder<'a> {
    scope: PatchScope<'a>,
}

impl<'a> MapEncoder<'a> {
    fn new(parent: &'a mut Encoder) -> Self {
        Self { scope: PatchScope::new(parent) }
    }

    #[must_use]
    pub fn key(&mut self, k: &str) -> Result<MapValueEncoder<'_>> {
        self.scope.parent.str(k)?;
        Ok(MapValueEncoder {
            parent: self.scope.parent,
        })
    }

    pub fn finish(self) -> Result<&'a mut Encoder> {
        self.scope.finish()
    }
}

#[must_use]
pub struct MapValueEncoder<'a> {
    parent: &'a mut Encoder,
}

impl<'a> MapValueEncoder<'a> {
    encode_wrapper_api!([self], (), 'a;
        parent: self.parent;
        pre: {};
        post: ()
    );
}

pub struct RecordEncoder<'a> {
    scope: PatchScope<'a>,
}

impl<'a> RecordEncoder<'a> {
    pub fn bytes(&mut self, data: &[u8]) -> Result<&mut Self> {
        self.scope.parent.buf.extend_from_slice(data);
        Ok(self)
    }

    #[inline]
    pub fn bool(&mut self, v: bool) -> Result<&mut Self> {
        self.scope.parent.write_tag(Tag::Bool);
        self.scope.parent.buf.push(v as u8);
        Ok(self)
    }

    #[inline]
    pub fn u8(&mut self, v: u8) -> Result<&mut Self> {
        self.scope.parent.write_tag(Tag::U8);
        self.scope.parent.buf.push(v);
        Ok(self)
    }

    #[inline]
    pub fn i8(&mut self, v: i8) -> Result<&mut Self> {
        self.scope.parent.write_tag(Tag::S8);
        self.scope.parent.buf.push(v as u8);
        Ok(self)
    }

    for_each_multibyte_scalar!(encode_record_multibyte, ());

    pub fn finish(self) -> Result<&'a mut Encoder> {
        self.scope.finish()
    }
}

pub struct ArrayEncoder<'a> {
    scope: PatchScope<'a>,
    stride: usize,
}

impl<'a> ArrayEncoder<'a> {
    pub unsafe fn push_unchecked(&mut self, data: &[u8]) -> Result<()> {
        self.scope.parent.buf.extend_from_slice(data);
        Ok(())
    }

    pub fn push(&mut self, data: &[u8]) -> Result<()> {
        if data.len() != self.stride {
            return Err(Error::Malformed);
        }
        unsafe { self.push_unchecked(data) }
    }

    #[inline]
    pub fn bool(&mut self, v: bool) -> Result<()> {
        self.scope.parent.write_tag(Tag::Bool);
        self.scope.parent.buf.push(v as u8);
        Ok(())
    }

    #[inline]
    pub fn u8(&mut self, v: u8) -> Result<()> {
        self.scope.parent.write_tag(Tag::U8);
        self.scope.parent.buf.push(v);
        Ok(())
    }

    #[inline]
    pub fn i8(&mut self, v: i8) -> Result<()> {
        self.scope.parent.write_tag(Tag::S8);
        self.scope.parent.buf.push(v as u8);
        Ok(())
    }

    for_each_multibyte_scalar!(encode_array_multibyte, ());

    /// Starts writing a fixed-size record into the array.
    pub fn record(&mut self) -> RecordBodyEncoder<'_, 'a> {
        let start = self.scope.parent.buf.len();
        RecordBodyEncoder {
            parent: self,
            start,
        }
    }

    pub fn finish(self) -> Result<&'a mut Encoder> {
        self.scope.finish()
    }
}

pub struct RecordBodyEncoder<'p, 'a> {
    parent: &'p mut ArrayEncoder<'a>,
    start: usize,
}

impl<'p, 'a> RecordBodyEncoder<'p, 'a> {
    pub fn bytes(&mut self, data: &[u8]) -> Result<&mut Self> {
        // We bypass stride checks until finish
        unsafe { self.parent.push_unchecked(data)?; }
        Ok(self)
    }

    #[inline]
    pub fn bool(&mut self, v: bool) -> Result<&mut Self> {
        self.parent.scope.parent.write_tag(Tag::Bool);
        self.parent.scope.parent.buf.push(v as u8);
        Ok(self)
    }

    #[inline]
    pub fn u8(&mut self, v: u8) -> Result<&mut Self> {
        self.parent.scope.parent.write_tag(Tag::U8);
        self.parent.scope.parent.buf.push(v);
        Ok(self)
    }

    #[inline]
    pub fn i8(&mut self, v: i8) -> Result<&mut Self> {
        self.parent.scope.parent.write_tag(Tag::S8);
        self.parent.scope.parent.buf.push(v as u8);
        Ok(self)
    }

    for_each_multibyte_scalar!(encode_record_multibyte, ());

    pub fn finish(self) -> Result<&'p mut ArrayEncoder<'a>>
    where
        'a: 'p,
    {
        let end = self.parent.scope.parent.buf.len();
        let written = end - self.start;
        if written != self.parent.stride {
            return Err(Error::Malformed);
        }
        Ok(self.parent)
    }
}
