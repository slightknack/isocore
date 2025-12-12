use crate::types::Result;
use crate::types::Error;
use crate::types::Tag;
use crate::cursor::Cursor;
use crate::macros::impl_from_bytes;
use crate::macros::decode_array_method;
use crate::macros::decode_val_as;
use crate::macros::decode_expect_tag;
use crate::macros::decode_record_prim;
use crate::macros::for_each_scalar;

pub(crate) trait FromBytes: Sized + Copy {
    const SIZE: usize;
    fn read_from(src: &[u8]) -> Self;
}

impl FromBytes for u8 {
    const SIZE: usize = 1;
    #[inline(always)] fn read_from(src: &[u8]) -> Self { src[0] }
}
impl FromBytes for i8 {
    const SIZE: usize = 1;
    #[inline(always)] fn read_from(src: &[u8]) -> Self { src[0] as i8 }
}
impl FromBytes for bool {
    const SIZE: usize = 1;
    #[inline(always)] fn read_from(src: &[u8]) -> Self { src[0] != 0 }
}

impl_from_bytes!(u16, 2); impl_from_bytes!(i16, 2);
impl_from_bytes!(u32, 4); impl_from_bytes!(i32, 4);
impl_from_bytes!(u64, 8); impl_from_bytes!(i64, 8);
impl_from_bytes!(f32, 4); impl_from_bytes!(f64, 8);

#[derive(Debug, Clone)]
pub struct Decoder<'a> {
    cursor: Cursor<'a>,
}

impl<'a> Decoder<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self {
            cursor: Cursor::new(buf),
        }
    }

    pub fn with_cursor(cursor: Cursor<'a>) -> Self {
        Self { cursor }
    }

    pub fn cursor(&self) -> &Cursor<'a> {
        &self.cursor
    }

    pub fn cursor_mut(&mut self) -> &mut Cursor<'a> {
        &mut self.cursor
    }

    pub fn pos(&self) -> usize {
        self.cursor.pos()
    }

    pub fn remaining(&self) -> usize {
        self.cursor.remaining()
    }

    fn read_primitive<T: FromBytes>(&mut self) -> Result<T> {
        let bytes = self.cursor.read_bytes(T::SIZE)?;
        Ok(T::read_from(bytes))
    }

    pub fn read_tag(&mut self) -> Result<Tag> {
        let byte = self.cursor.read_byte()?;
        Tag::from_u8(byte).ok_or(Error::InvalidTag(byte))
    }

    pub fn peek_tag(&self) -> Result<Tag> {
        let byte = self.cursor.peek_byte()?;
        Tag::from_u8(byte).ok_or(Error::InvalidTag(byte))
    }

    for_each_scalar!(decode_expect_tag, ());

    pub fn str(&mut self) -> Result<&'a str> {
        self.expect_blob(Tag::String, |bytes| {
            std::str::from_utf8(bytes).map_err(|_| Error::InvalidUtf8)
        })
    }

    pub fn bytes(&mut self) -> Result<&'a [u8]> {
        self.expect_blob(Tag::Bytes, |b| Ok(b))
    }

    pub fn record_raw(&mut self) -> Result<&'a [u8]> {
        self.expect_blob(Tag::Struct, |b| Ok(b))
    }

    fn expect_blob<F, T>(&mut self, expected: Tag, f: F) -> Result<T>
    where
        F: FnOnce(&'a [u8]) -> Result<T>,
    {
        let tag = self.read_tag()?;
        if tag != expected {
            return Err(Error::TypeMismatch);
        }
        let len: u32 = self.read_primitive()?;
        let bytes = self.cursor.read_bytes(len as usize)?;
        f(bytes)
    }

    pub fn value(&mut self) -> Result<ValueDecoder<'a>> {
        ValueDecoder::read(self)
    }

    pub fn skip_value(&mut self) -> Result<()> {
        let tag = self.read_tag()?;
        match tag {
            Tag::Bool | Tag::U8 | Tag::S8 => self.cursor.skip(1),
            Tag::U16 | Tag::S16 => self.cursor.skip(2),
            Tag::U32 | Tag::S32 | Tag::F32 => self.cursor.skip(4),
            Tag::U64 | Tag::S64 | Tag::F64 => self.cursor.skip(8),

            // ADT tags with no payload
            Tag::Unit | Tag::OptionSome | Tag::OptionNone | Tag::ResultOk | Tag::ResultErr => {
                Ok(())
            }

            // Variant has a string payload
            Tag::Variant => {
                let len: u32 = self.read_primitive()?;
                self.cursor.skip(len as usize)
            }

            Tag::String | Tag::Bytes | Tag::Struct |
            Tag::List | Tag::Map | Tag::Array => {
                let len: u32 = self.read_primitive()?;
                self.cursor.skip(len as usize)
            }
        }
    }

    /// Extract the raw bytes for the next value without decoding it
    pub fn raw_value(&mut self) -> Result<&'a [u8]> {
        let start_pos = self.cursor.pos();
        self.skip_value()?;
        let end_pos = self.cursor.pos();
        let slice = self.cursor.full_slice();
        Ok(&slice[start_pos..end_pos])
    }

    pub fn list(&mut self) -> Result<ListDecoder<'a>> {
        let tag = self.read_tag()?;
        if tag != Tag::List {
            return Err(Error::TypeMismatch);
        }
        let byte_len: u32 = self.read_primitive()?;
        let bytes = self.cursor.read_bytes(byte_len as usize)?;

        Ok(ListDecoder {
            cursor: Cursor::new(bytes),
            end_pos: bytes.len(),
        })
    }

    pub fn map(&mut self) -> Result<MapDecoder<'a>> {
        let tag = self.read_tag()?;
        if tag != Tag::Map {
            return Err(Error::TypeMismatch);
        }
        let byte_len: u32 = self.read_primitive()?;
        let bytes = self.cursor.read_bytes(byte_len as usize)?;

        Ok(MapDecoder {
            cursor: Cursor::new(bytes),
            end_pos: bytes.len(),
        })
    }

    pub fn array(&mut self) -> Result<ArrayDecoder<'a>> {
        let tag = self.read_tag()?;
        if tag != Tag::Array {
            return Err(Error::TypeMismatch);
        }
        let byte_len: u32 = self.read_primitive()?;
        let bytes = self.cursor.read_bytes(byte_len as usize)?;

        let mut inner = Cursor::new(bytes);
        let item_tag_byte = inner.read_byte()?;
        let item_tag = Tag::from_u8(item_tag_byte).ok_or(Error::InvalidTag(item_tag_byte))?;
        let stride_bytes = inner.read_bytes(4)?;
        let stride = u32::from_le_bytes([stride_bytes[0], stride_bytes[1], stride_bytes[2], stride_bytes[3]]) as usize;

        let header_size = 5;
        let payload_len = bytes.len().saturating_sub(header_size);

        if stride == 0 || payload_len % stride != 0 { return Err(Error::Malformed); }
        let count = payload_len / stride;

        Ok(ArrayDecoder {
            cursor: inner,
            item_tag,
            stride,
            remaining: count,
        })
    }

    pub fn record(&mut self) -> Result<RecordDecoder<'a>> {
        let bytes = self.record_raw()?;
        Ok(RecordDecoder::new(bytes))
    }
}

#[derive(Debug)]
pub struct ListDecoder<'a> {
    cursor: Cursor<'a>,
    end_pos: usize,
}

impl<'a> ListDecoder<'a> {
    pub fn next(&mut self) -> Result<Option<ValueDecoder<'a>>> {
        if self.cursor.pos() >= self.end_pos {
            return Ok(None);
        }
        let mut decoder = Decoder::with_cursor(self.cursor.clone());
        let value = ValueDecoder::read(&mut decoder)?;
        self.cursor = decoder.cursor;
        Ok(Some(value))
    }
}

#[derive(Debug)]
pub struct MapDecoder<'a> {
    cursor: Cursor<'a>,
    end_pos: usize,
}

impl<'a> MapDecoder<'a> {
    pub fn next(&mut self) -> Result<Option<(&'a str, ValueDecoder<'a>)>> {
        if self.cursor.pos() >= self.end_pos {
            return Ok(None);
        }

        let mut decoder = Decoder::with_cursor(self.cursor.clone());
        
        let tag = decoder.read_tag()?;
        if tag != Tag::String { return Err(Error::TypeMismatch); }
        let k_len: u32 = decoder.read_primitive()?;
        let k_bytes = decoder.cursor.read_bytes(k_len as usize)?;
        let key = std::str::from_utf8(k_bytes).map_err(|_| Error::InvalidUtf8)?;

        let val = ValueDecoder::read(&mut decoder)?;
        self.cursor = decoder.cursor;
        Ok(Some((key, val)))
    }
}

#[derive(Debug)]
pub struct ArrayDecoder<'a> {
    cursor: Cursor<'a>,
    item_tag: Tag,
    stride: usize,
    remaining: usize,
}

impl<'a> ArrayDecoder<'a> {
    pub fn item_tag(&self) -> Tag { self.item_tag }
    pub fn stride(&self) -> usize { self.stride }
    pub fn remaining(&self) -> usize { self.remaining }

    pub fn next(&mut self) -> Result<Option<ValueDecoder<'a>>> {
        if self.remaining == 0 { return Ok(None); }
        self.remaining -= 1;

        let bytes = self.cursor.read_bytes(self.stride)?;
        let value = ValueDecoder::from_untagged_bytes(self.item_tag, bytes)?;

        Ok(Some(value))
    }

    pub fn skip_all(&mut self) -> Result<()> {
        if self.remaining > 0 {
            let skip = self.remaining * self.stride;
            self.cursor.skip(skip)?;
            self.remaining = 0;
        }
        Ok(())
    }

    for_each_scalar!(decode_array_method, ());
}

#[derive(Debug)]
pub enum ValueDecoder<'a> {
    Bool(bool),
    U8(u8),
    S8(i8),
    U16(u16),
    S16(i16),
    U32(u32),
    S32(i32),
    U64(u64),
    S64(i64),
    F32(f32),
    F64(f64),
    Bytes(&'a [u8]),
    Struct(&'a [u8]),
    Str(&'a str),
    List(ListDecoder<'a>),
    Map(MapDecoder<'a>),
    Array(ArrayDecoder<'a>),
    // ADT variants
    Unit,
    OptionSome,
    OptionNone,
    ResultOk,
    ResultErr,
    Variant(&'a str),
}

impl<'a> ValueDecoder<'a> {
    pub fn from_untagged_bytes(tag: Tag, bytes: &'a [u8]) -> Result<Self> {
        use ValueDecoder::*;
        match tag {
            Tag::Bool => Ok(Bool(FromBytes::read_from(bytes))),
            Tag::U8   => Ok(U8(FromBytes::read_from(bytes))),
            Tag::S8   => Ok(S8(FromBytes::read_from(bytes))),
            Tag::U16  => Ok(U16(FromBytes::read_from(bytes))),
            Tag::S16  => Ok(S16(FromBytes::read_from(bytes))),
            Tag::U32  => Ok(U32(FromBytes::read_from(bytes))),
            Tag::S32  => Ok(S32(FromBytes::read_from(bytes))),
            Tag::U64  => Ok(U64(FromBytes::read_from(bytes))),
            Tag::S64  => Ok(S64(FromBytes::read_from(bytes))),
            Tag::F32  => Ok(F32(FromBytes::read_from(bytes))),
            Tag::F64  => Ok(F64(FromBytes::read_from(bytes))),

            Tag::Bytes => Ok(Bytes(bytes)),
            Tag::Struct => Ok(Struct(bytes)),

            Tag::String => {
                let s = std::str::from_utf8(bytes).map_err(|_| Error::InvalidUtf8)?;
                Ok(ValueDecoder::Str(s))
            }

            Tag::List => {
                Ok(List(ListDecoder {
                    cursor: Cursor::new(bytes),
                    end_pos: bytes.len(),
                }))
            }

            Tag::Map => {
                Ok(Map(MapDecoder {
                    cursor: Cursor::new(bytes),
                    end_pos: bytes.len(),
                }))
            }

            Tag::Array => {
                let mut inner = Cursor::new(bytes);
                let item_tag_byte = inner.read_byte()?;
                let item_tag = Tag::from_u8(item_tag_byte).ok_or(Error::InvalidTag(item_tag_byte))?;
                let stride_bytes = inner.read_bytes(4)?;
                let stride = u32::from_le_bytes([stride_bytes[0], stride_bytes[1], stride_bytes[2], stride_bytes[3]]) as usize;

                let header_size = 5;
                let payload_len = bytes.len().saturating_sub(header_size);

                if stride == 0 || payload_len % stride != 0 { return Err(Error::Malformed); }
                let count = payload_len / stride;

                Ok(Array(ArrayDecoder {
                    cursor: inner,
                    item_tag,
                    stride,
                    remaining: count,
                }))
            }

            // ADT tags - these should not appear in from_untagged_bytes
            // as they have no payload (handled in read())
            Tag::Unit => Ok(Unit),
            Tag::OptionSome => Ok(OptionSome),
            Tag::OptionNone => Ok(OptionNone),
            Tag::ResultOk => Ok(ResultOk),
            Tag::ResultErr => Ok(ResultErr),
            Tag::Variant => {
                // Variant payload is a string
                let s = std::str::from_utf8(bytes).map_err(|_| Error::InvalidUtf8)?;
                Ok(ValueDecoder::Variant(s))
            }
        }
    }

    pub fn read(decoder: &mut Decoder<'a>) -> Result<Self> {
        let tag = decoder.read_tag()?;

        // ADT tags with no payload
        match tag {
            Tag::Unit => return Ok(ValueDecoder::Unit),
            Tag::OptionSome => return Ok(ValueDecoder::OptionSome),
            Tag::OptionNone => return Ok(ValueDecoder::OptionNone),
            Tag::ResultOk => return Ok(ValueDecoder::ResultOk),
            Tag::ResultErr => return Ok(ValueDecoder::ResultErr),
            Tag::Variant => {
                // Variant has a string name as payload
                let len = decoder.read_primitive::<u32>()? as usize;
                let bytes = decoder.cursor.read_bytes(len)?;
                let name = std::str::from_utf8(bytes).map_err(|_| Error::InvalidUtf8)?;
                return Ok(ValueDecoder::Variant(name));
            }
            _ => {}
        }

        let len = match tag {
            Tag::Bool | Tag::U8 | Tag::S8 => 1,
            Tag::U16 | Tag::S16 => 2,
            Tag::U32 | Tag::S32 | Tag::F32 => 4,
            Tag::U64 | Tag::S64 | Tag::F64 => 8,

            Tag::String | Tag::Bytes | Tag::Struct |
            Tag::List | Tag::Map | Tag::Array => {
                decoder.read_primitive::<u32>()? as usize
            }

            _ => return Err(Error::InvalidTag(tag as u8)),
        };

        let bytes = decoder.cursor.read_bytes(len)?;
        Self::from_untagged_bytes(tag, bytes)
    }

    for_each_scalar!(decode_val_as, ());

    pub fn as_str(&self) -> Result<&'a str> {
        match self { ValueDecoder::Str(v) => Ok(*v), _ => Err(Error::TypeMismatch) }
    }

    pub fn as_bytes(&self) -> Result<&'a [u8]> {
        match self { ValueDecoder::Bytes(v) => Ok(*v), _ => Err(Error::TypeMismatch) }
    }
}

pub struct RecordDecoder<'a> {
    cursor: Cursor<'a>,
    end: usize,
}

impl<'a> RecordDecoder<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            end: data.len(),
            cursor: Cursor::new(data),
        }
    }

    pub fn new_unchecked(data: &'a [u8]) -> Self {
        Self {
            end: data.len(),
            cursor: Cursor::new(data),
        }
    }

    pub fn remaining(&self) -> usize {
        self.cursor.remaining()
    }

    pub fn raw(&self) -> &'a [u8] {
        self.cursor.as_slice()
    }

    fn read_primitive<T: FromBytes>(&mut self) -> Result<T> {
        let bytes = self.cursor.read_bytes(T::SIZE)?;
        Ok(T::read_from(bytes))
    }

    for_each_scalar!(decode_record_prim, ());

    pub fn bytes(&mut self, len: usize) -> Result<&'a [u8]> {
        self.cursor.read_bytes(len)
    }
}

impl<'a> Drop for RecordDecoder<'a> {
    fn drop(&mut self) {
        if self.cursor.pos() != self.end {
            debug_assert!(false, "RecordReader dropped with unread bytes");
        }
    }
}
