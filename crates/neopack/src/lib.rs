//! # Neopack
//!
//! A distinctively simple, bounded, and schema-agnostic serialization library.
//!
//! ## Philosophy
//!
//! - **TigerStyle**: Safety through simplicity and explicit state. No hidden buffers, no recursion limits.
//! - **TLV Architecture**: `[Tag][Length?][Value]` structure enables safe skipping of unknown fields.
//! - **Bounded**: Encoders track state explicitly. Decoders are zero-copy, bounds-checked views.
//!
//! ## Format
//!
//! - **Scalars**: `[Tag: 1b][Data: N]`
//! - **Blobs**: `[Tag: 1b][Len: 4b][Data: Len]`
//! - **Containers**: `[Tag: 1b][Len: 4b][Body: Len]`
//!
//! All integers are Little-Endian.

#[cfg(test)]
mod tests;

/// Neopack serialization and deserialization errors.
#[derive(Debug, Clone)]
pub enum Error {
    /// Internal buffer capacity exceeded.
    BufferFull,
    /// Byte does not correspond to a valid Neopack `Tag`.
    InvalidTag(u8),
    /// String data is not valid UTF-8.
    InvalidUtf8,
    /// Closing a scope that does not match the active scope stack.
    ScopeMismatch { expected: Scope, actual: Scope },
    /// Attempted to close a scope when only the Root remains.
    ScopeUnderflow,
    /// Attempted to finalize the buffer with open scopes.
    ScopeStillOpen,
    /// Buffer exhausted while reading.
    UnexpectedEnd,
    /// Blob or container length exceeds `u32::MAX`.
    BlobTooLarge(usize),
    /// Structural Violation: Attempted to write >1 item into a strict scope (Option/Result/Variant).
    TooManyItems(Scope),
    /// Structural Violation: Attempted to close a strict scope (Option/Result/Variant) without a value.
    EmptyAdt(Scope),
    /// Structural Violation: Attempted to write a non-Variant directly into a Map.
    InvalidMapEntry,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::InvalidTag(b) => write!(f, "Invalid Tag byte: {:#04x}", b),
            Error::ScopeMismatch { expected, actual } => {
                write!(f, "Scope Mismatch: expected {:?}, found {:?}", expected, actual)
            }
            Error::TooManyItems(s) => write!(f, "Too many items in scope {:?}; expected exactly 1", s),
            Error::EmptyAdt(s) => write!(f, "Empty ADT scope {:?}; expected exactly 1 item", s),
            _ => write!(f, "{:?}", self),
        }
    }
}

impl std::error::Error for Error {}

/// Specialized `Result` for Neopack operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Identifies the type of the encoded value.
///
/// Used for schema evolution and safe skipping of unknown fields.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    /// Padding/Alignment (Skip).
    Pad = 0x00,

    // Fixed-width scalars
    BoolTrue = 0x01,
    BoolFalse = 0x02,
    U8 = 0x03,
    U16 = 0x04,
    U32 = 0x05,
    U64 = 0x06,
    S8 = 0x07,
    S16 = 0x08,
    S32 = 0x09,
    S64 = 0x0A,
    F32 = 0x0B,
    F64 = 0x0C,
    Char = 0x0D,

    // Unit / Void
    Unit = 0x0E,
    OptionNone = 0x0F,

    // Blobs (Tag + u32 Len + Bytes)
    String = 0x10,
    Bytes = 0x11,

    // Containers (Tag + u32 Len + Body)
    List = 0x20,
    Map = 0x21,

    // ADTs (Tag + u32 Len + Body)
    OptionSome = 0x30,
    ResultOk = 0x31,
    ResultErr = 0x32,
    Variant = 0x33,
}

impl Tag {
    /// Returns the Tag variant for a given byte, or `None` if invalid.
    pub fn from_u8(b: u8) -> Option<Self> {
        match b {
            0x00 => Some(Tag::Pad),
            0x01 => Some(Tag::BoolTrue),
            0x02 => Some(Tag::BoolFalse),
            0x03 => Some(Tag::U8),
            0x04 => Some(Tag::U16),
            0x05 => Some(Tag::U32),
            0x06 => Some(Tag::U64),
            0x07 => Some(Tag::S8),
            0x08 => Some(Tag::S16),
            0x09 => Some(Tag::S32),
            0x0A => Some(Tag::S64),
            0x0B => Some(Tag::F32),
            0x0C => Some(Tag::F64),
            0x0D => Some(Tag::Char),
            0x0E => Some(Tag::Unit),
            0x0F => Some(Tag::OptionNone),
            0x10 => Some(Tag::String),
            0x11 => Some(Tag::Bytes),
            0x20 => Some(Tag::List),
            0x21 => Some(Tag::Map),
            0x30 => Some(Tag::OptionSome),
            0x31 => Some(Tag::ResultOk),
            0x32 => Some(Tag::ResultErr),
            0x33 => Some(Tag::Variant),
            _ => None,
        }
    }
}

/// Internal state tracking for the `Encoder` stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The virtual root; allows any item.
    Root,
    /// Ordered sequence; allows any number of items.
    List,
    /// Key-Value container; strictly allows only `Tag::Variant` items.
    Map,
    /// Strict container; allows exactly one item.
    Option,
    /// Strict container; allows exactly one item.
    Result,
    /// Strict container; allows exactly one item (the payload) after the name.
    Variant,
}



/// An active container scope on the `Encoder` stack.
struct Frame {
    start: usize,
    scope: Scope,
    count: usize,
}

/// A bounded, state-machine driven encoder.
///
/// The Encoder maintains a stack of open scopes to enforce structural strictness
/// and automatically back-patch length headers.
///
/// # Structural Invariants
///
/// All write methods validate the operation against the current `Scope`.
/// Returns an `Error` if the write violates the following rules:
///
/// 1.  **Map Scopes**: Only `Tag::Variant` items may be written.
/// 2.  **ADT Scopes (Option, Result, Variant)**: Exactly one item must be written.
///     Attempts to write >1 item or close the scope with 0 items will fail.
/// 3.  **Root Scope**: The encoder must end in the Root scope to finalize bytes.
pub struct Encoder {
    buf: Vec<u8>,
    /// Bottom is always `Scope::Root`.
    stack: Vec<Frame>,
}

impl Encoder {
    /// Creates a new encoder with default capacity.
    pub fn new() -> Self {
        let mut enc = Self {
            buf: Vec::with_capacity(1024),
            stack: Vec::with_capacity(8),
        };
        enc.stack.push(Frame { start: 0, scope: Scope::Root, count: 0 });
        enc
    }

    /// Consumes the encoder and returns the final byte vector.
    ///
    /// # Errors
    /// Returns `Error::ScopeStillOpen` if the stack depth > 1.
    pub fn into_bytes(self) -> Result<Vec<u8>> {
        if self.stack.len() > 1 {
            return Err(Error::ScopeStillOpen);
        }
        Ok(self.buf)
    }

    /// Returns a view of the current buffer.
    ///
    /// # Errors
    /// Returns `Error::ScopeStillOpen` if the stack depth > 1.
    pub fn as_bytes(&self) -> Result<&[u8]> {
        if self.stack.len() > 1 {
            return Err(Error::ScopeStillOpen);
        }
        Ok(&self.buf)
    }

    fn current_frame(&mut self) -> &mut Frame {
        self.stack.last_mut().unwrap()
    }

    fn check_write(&mut self, tag: Tag) -> Result<()> {
        let frame = self.current_frame();
        match frame.scope {
            Scope::Root | Scope::List => Ok(()),
            Scope::Map => {
                if tag != Tag::Variant {
                    Err(Error::InvalidMapEntry)
                } else {
                    Ok(())
                }
            },
            Scope::Option | Scope::Result | Scope::Variant => {
                if frame.count >= 1 {
                    Err(Error::TooManyItems(frame.scope))
                } else {
                    Ok(())
                }
            }
        }
    }

    fn on_item_written(&mut self) {
        let frame = self.current_frame();
        frame.count += 1;
    }

    fn write_tag(&mut self, tag: Tag) -> Result<()> {
        self.check_write(tag)?;
        self.buf.push(tag as u8);
        Ok(())
    }

    fn write_u32_raw(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn begin_scope(&mut self, tag: Tag, scope: Scope) -> Result<()> {
        self.check_write(tag)?;

        self.buf.push(tag as u8);
        self.buf.extend_from_slice(&[0, 0, 0, 0]); // Length placeholder

        self.stack.push(Frame {
            start: self.buf.len(), // Body starts after Length
            scope,
            count: 0,
        });
        Ok(())
    }

    fn end_scope(&mut self, expected: Scope) -> Result<()> {
        if self.stack.len() <= 1 {
            return Err(Error::ScopeUnderflow);
        }

        { // Validate Scope State
            let frame = self.current_frame();
            if frame.scope != expected {
                return Err(Error::ScopeMismatch { expected, actual: frame.scope });
            }

            match frame.scope {
                Scope::Option | Scope::Result | Scope::Variant => {
                    if frame.count == 0 {
                        return Err(Error::EmptyAdt(frame.scope));
                    }
                },
                _ => {}
            }
        }

        // Pop and Patch
        let frame = self.stack.pop().unwrap();
        let body_len = self.buf.len() - frame.start;

        if body_len > u32::MAX as usize {
            return Err(Error::BlobTooLarge(body_len));
        }

        let len_bytes = (body_len as u32).to_le_bytes();
        let len_pos = frame.start - 4;
        self.buf[len_pos..frame.start].copy_from_slice(&len_bytes);

        self.on_item_written();

        Ok(())
    }

    /// Encodes a boolean value.
    pub fn bool(&mut self, v: bool) -> Result<()> {
        self.write_tag(if v { Tag::BoolTrue } else { Tag::BoolFalse })?;
        self.on_item_written();
        Ok(())
    }

    /// Encodes an unsigned 8-bit integer.
    pub fn u8(&mut self, v: u8) -> Result<()> { self.write_tag(Tag::U8)?; self.buf.push(v); self.on_item_written(); Ok(()) }
    /// Encodes a signed 8-bit integer.
    pub fn s8(&mut self, v: i8) -> Result<()> { self.write_tag(Tag::S8)?; self.buf.push(v as u8); self.on_item_written(); Ok(()) }

    /// Encodes an unsigned 16-bit integer (LE).
    pub fn u16(&mut self, v: u16) -> Result<()> { self.write_tag(Tag::U16)?; self.buf.extend_from_slice(&v.to_le_bytes()); self.on_item_written(); Ok(()) }
    /// Encodes a signed 16-bit integer (LE).
    pub fn s16(&mut self, v: i16) -> Result<()> { self.write_tag(Tag::S16)?; self.buf.extend_from_slice(&v.to_le_bytes()); self.on_item_written(); Ok(()) }

    /// Encodes an unsigned 32-bit integer (LE).
    pub fn u32(&mut self, v: u32) -> Result<()> { self.write_tag(Tag::U32)?; self.buf.extend_from_slice(&v.to_le_bytes()); self.on_item_written(); Ok(()) }
    /// Encodes a signed 32-bit integer (LE).
    pub fn s32(&mut self, v: i32) -> Result<()> { self.write_tag(Tag::S32)?; self.buf.extend_from_slice(&v.to_le_bytes()); self.on_item_written(); Ok(()) }

    /// Encodes an unsigned 64-bit integer (LE).
    pub fn u64(&mut self, v: u64) -> Result<()> { self.write_tag(Tag::U64)?; self.buf.extend_from_slice(&v.to_le_bytes()); self.on_item_written(); Ok(()) }
    /// Encodes a signed 64-bit integer (LE).
    pub fn s64(&mut self, v: i64) -> Result<()> { self.write_tag(Tag::S64)?; self.buf.extend_from_slice(&v.to_le_bytes()); self.on_item_written(); Ok(()) }

    /// Encodes a 32-bit float (LE).
    pub fn f32(&mut self, v: f32) -> Result<()> { self.write_tag(Tag::F32)?; self.buf.extend_from_slice(&v.to_le_bytes()); self.on_item_written(); Ok(()) }
    /// Encodes a 64-bit float (LE).
    pub fn f64(&mut self, v: f64) -> Result<()> { self.write_tag(Tag::F64)?; self.buf.extend_from_slice(&v.to_le_bytes()); self.on_item_written(); Ok(()) }

    /// Encodes a char as u32 (LE).
    pub fn char(&mut self, v: char) -> Result<()> { self.write_tag(Tag::Char)?; self.buf.extend_from_slice(&(v as u32).to_le_bytes()); self.on_item_written(); Ok(()) }

    /// Encodes Unit `()`.
    pub fn unit(&mut self) -> Result<()> { self.write_tag(Tag::Unit)?; self.on_item_written(); Ok(()) }
    /// Encodes `Option::None`.
    pub fn option_none(&mut self) -> Result<()> { self.write_tag(Tag::OptionNone)?; self.on_item_written(); Ok(()) }

    /// Encodes a UTF-8 string blob.
    pub fn str(&mut self, v: &str) -> Result<()> {
        let len = v.len();
        if len > u32::MAX as usize { return Err(Error::BlobTooLarge(len)); }
        self.write_tag(Tag::String)?;
        self.write_u32_raw(len as u32);
        self.buf.extend_from_slice(v.as_bytes());
        self.on_item_written();
        Ok(())
    }

    /// Encodes a raw byte blob.
    pub fn bytes(&mut self, v: &[u8]) -> Result<()> {
        let len = v.len();
        if len > u32::MAX as usize { return Err(Error::BlobTooLarge(len)); }
        self.write_tag(Tag::Bytes)?;
        self.write_u32_raw(len as u32);
        self.buf.extend_from_slice(v);
        self.on_item_written();
        Ok(())
    }

    /// Begins a List container.
    ///
    /// # Invariants
    /// - Must be closed via `list_end()`.
    /// - Allows any number of items.
    pub fn list_begin(&mut self) -> Result<()> { self.begin_scope(Tag::List, Scope::List) }
    /// Ends a List container.
    pub fn list_end(&mut self) -> Result<()> { self.end_scope(Scope::List) }

    /// Begins a Map container.
    ///
    /// # Invariants
    /// - Must be closed via `map_end()`.
    /// - **Strict:** Only `variant_begin()` (Key/Value pair) is allowed as a direct child.
    pub fn map_begin(&mut self) -> Result<()> { self.begin_scope(Tag::Map, Scope::Map) }
    /// Ends a Map container.
    pub fn map_end(&mut self) -> Result<()> { self.end_scope(Scope::Map) }

    /// Begins an `Option::Some` container.
    ///
    /// # Invariants
    /// - Must be closed via `option_some_end()`.
    /// - **Strict:** Requires exactly one item to be written.
    pub fn option_some_begin(&mut self) -> Result<()> { self.begin_scope(Tag::OptionSome, Scope::Option) }
    /// Ends an `Option::Some` container.
    pub fn option_some_end(&mut self) -> Result<()> { self.end_scope(Scope::Option) }

    /// Begins a `Result::Ok` container.
    ///
    /// # Invariants
    /// - Must be closed via `result_ok_end()`.
    /// - **Strict:** Requires exactly one item to be written.
    pub fn result_ok_begin(&mut self) -> Result<()> { self.begin_scope(Tag::ResultOk, Scope::Result) }
    /// Ends a `Result::Ok` container.
    pub fn result_ok_end(&mut self) -> Result<()> { self.end_scope(Scope::Result) }

    /// Begins a `Result::Err` container.
    ///
    /// # Invariants
    /// - Must be closed via `result_err_end()`.
    /// - **Strict:** Requires exactly one item to be written.
    pub fn result_err_begin(&mut self) -> Result<()> { self.begin_scope(Tag::ResultErr, Scope::Result) }
    /// Ends a `Result::Err` container.
    pub fn result_err_end(&mut self) -> Result<()> { self.end_scope(Scope::Result) }

    /// Begins a Variant (Named Payload).
    ///
    /// Encodes the name string immediately.
    ///
    /// # Invariants
    /// - Must be closed via `variant_end()`.
    /// - **Strict:** Requires exactly one item (the payload) to be written after this call.
    pub fn variant_begin(&mut self, name: &str) -> Result<()> {
        self.begin_scope(Tag::Variant, Scope::Variant)?;
        // Write Name (metadata, not payload)
        self.str(name)?;
        // Reset count; user must write exactly one payload item next.
        self.current_frame().count = 0;
        Ok(())
    }
    /// Ends a Variant.
    pub fn variant_end(&mut self) -> Result<()> { self.end_scope(Scope::Variant) }
}

/// A zero-copy, bounds-checked cursor over a byte slice.
///
/// Decoders are immutable views. Reading advances the internal cursor.
/// Container reads return new `Decoder` instances restricted to the container's body.
///
/// # Errors
/// All read operations return `Error::UnexpectedEnd` if the buffer is exhausted.
#[derive(Debug, Clone)]
pub struct Decoder<'a> {
    buf: &'a [u8],
}

impl<'a> Decoder<'a> {
    /// Creates a decoder over the slice.
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    /// Returns the remaining bytes in the view.
    pub fn remaining(&self) -> usize {
        self.buf.len()
    }

    /// Peeks the next Tag without advancing.
    pub fn peek_tag(&self) -> Result<Tag> {
        if self.buf.is_empty() { return Err(Error::UnexpectedEnd); }
        Tag::from_u8(self.buf[0]).ok_or(Error::InvalidTag(self.buf[0]))
    }

    fn consume(&mut self, n: usize) -> Result<()> {
        if n > self.buf.len() { return Err(Error::UnexpectedEnd); }
        self.buf = &self.buf[n..];
        Ok(())
    }

    fn read_u8(&mut self) -> Result<u8> {
        if self.buf.is_empty() { return Err(Error::UnexpectedEnd); }
        let b = self.buf[0];
        self.buf = &self.buf[1..];
        Ok(b)
    }

    fn read_bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        if n > self.buf.len() { return Err(Error::UnexpectedEnd); }
        let (head, tail) = self.buf.split_at(n);
        self.buf = tail;
        Ok(head)
    }

    fn read_slice(&mut self, n: usize) -> Result<Decoder<'a>> {
        let bytes = self.read_bytes(n)?;
        Ok(Decoder::new(bytes))
    }

    fn check_tag(&mut self, expected: Tag) -> Result<()> {
        let tag = self.peek_tag()?;
        if tag == expected {
            self.consume(1)?;
            Ok(())
        } else {
            Err(Error::InvalidTag(tag as u8))
        }
    }

    /// Skips the next item and its nested children.
    pub fn skip(&mut self) -> Result<()> {
        let tag = self.peek_tag()?;
        self.consume(1)?; // Consume Tag

        match tag {
            Tag::Pad => {},
            Tag::BoolTrue | Tag::BoolFalse | Tag::Unit | Tag::OptionNone => {},

            // Fixed scalars
            Tag::U8 | Tag::S8 => { self.consume(1)?; },
            Tag::U16 | Tag::S16 => { self.consume(2)?; },
            Tag::U32 | Tag::S32 | Tag::F32 | Tag::Char => { self.consume(4)?; },
            Tag::U64 | Tag::S64 | Tag::F64 => { self.consume(8)?; },

            // Variable length (Blob or Scoped)
            // Structure: [Length: u32] [Body: Length]
            Tag::String | Tag::Bytes |
            Tag::List | Tag::Map |
            Tag::OptionSome | Tag::ResultOk | Tag::ResultErr | Tag::Variant => {
                let len_bytes = self.read_bytes(4)?;
                let len = u32::from_le_bytes(len_bytes.try_into().unwrap()) as usize;
                self.consume(len)?;
            }
        }
        Ok(())
    }

    /// Decodes a bool.
    pub fn bool(&mut self) -> Result<bool> {
        let tag = self.peek_tag()?;
        match tag {
            Tag::BoolTrue => { self.consume(1)?; Ok(true) },
            Tag::BoolFalse => { self.consume(1)?; Ok(false) },
            _ => Err(Error::InvalidTag(tag as u8))
        }
    }

    /// Decodes u8.
    pub fn u8(&mut self) -> Result<u8> { self.check_tag(Tag::U8)?; self.read_u8() }
    /// Decodes s8.
    pub fn s8(&mut self) -> Result<i8> { self.check_tag(Tag::S8)?; Ok(self.read_u8()? as i8) }

    /// Decodes u16 (LE).
    pub fn u16(&mut self) -> Result<u16> { self.check_tag(Tag::U16)?; Ok(u16::from_le_bytes(self.read_bytes(2)?.try_into().unwrap())) }
    /// Decodes s16 (LE).
    pub fn s16(&mut self) -> Result<i16> { self.check_tag(Tag::S16)?; Ok(i16::from_le_bytes(self.read_bytes(2)?.try_into().unwrap())) }

    /// Decodes u32 (LE).
    pub fn u32(&mut self) -> Result<u32> { self.check_tag(Tag::U32)?; Ok(u32::from_le_bytes(self.read_bytes(4)?.try_into().unwrap())) }
    /// Decodes s32 (LE).
    pub fn s32(&mut self) -> Result<i32> { self.check_tag(Tag::S32)?; Ok(i32::from_le_bytes(self.read_bytes(4)?.try_into().unwrap())) }

    /// Decodes u64 (LE).
    pub fn u64(&mut self) -> Result<u64> { self.check_tag(Tag::U64)?; Ok(u64::from_le_bytes(self.read_bytes(8)?.try_into().unwrap())) }
    /// Decodes s64 (LE).
    pub fn s64(&mut self) -> Result<i64> { self.check_tag(Tag::S64)?; Ok(i64::from_le_bytes(self.read_bytes(8)?.try_into().unwrap())) }

    /// Decodes f32 (LE).
    pub fn f32(&mut self) -> Result<f32> { self.check_tag(Tag::F32)?; Ok(f32::from_le_bytes(self.read_bytes(4)?.try_into().unwrap())) }
    /// Decodes f64 (LE).
    pub fn f64(&mut self) -> Result<f64> { self.check_tag(Tag::F64)?; Ok(f64::from_le_bytes(self.read_bytes(8)?.try_into().unwrap())) }

    /// Decodes char (u32 LE).
    pub fn char(&mut self) -> Result<char> {
        self.check_tag(Tag::Char)?;
        let val = u32::from_le_bytes(self.read_bytes(4)?.try_into().unwrap());
        std::char::from_u32(val).ok_or(Error::InvalidUtf8)
    }

    /// Decodes Unit `()`.
    pub fn unit(&mut self) -> Result<()> { self.check_tag(Tag::Unit) }
    /// Decodes `Option::None`.
    pub fn option_none(&mut self) -> Result<()> { self.check_tag(Tag::OptionNone) }

    /// Decodes a string slice (UTF-8).
    pub fn str(&mut self) -> Result<&'a str> {
        self.check_tag(Tag::String)?;
        let len = u32::from_le_bytes(self.read_bytes(4)?.try_into().unwrap()) as usize;
        let bytes = self.read_bytes(len)?;
        str::from_utf8(bytes).map_err(|_| Error::InvalidUtf8)
    }

    /// Decodes a byte slice.
    pub fn bytes(&mut self) -> Result<&'a [u8]> {
        self.check_tag(Tag::Bytes)?;
        let len = u32::from_le_bytes(self.read_bytes(4)?.try_into().unwrap()) as usize;
        self.read_bytes(len)
    }

    fn enter_container(&mut self, expected: Tag) -> Result<Decoder<'a>> {
        self.check_tag(expected)?;
        let len = u32::from_le_bytes(self.read_bytes(4)?.try_into().unwrap()) as usize;
        self.read_slice(len)
    }

    /// Decodes a List into an iterator.
    pub fn list(&mut self) -> Result<ListIter<'a>> {
        Ok(ListIter { dec: self.enter_container(Tag::List)? })
    }

    /// Decodes a Map into an iterator.
    pub fn map(&mut self) -> Result<MapIter<'a>> {
        Ok(MapIter { dec: self.enter_container(Tag::Map)? })
    }

    /// Decodes an Option.
    ///
    /// Returns `Some(Decoder)` for the payload if present, or `None`.
    pub fn option(&mut self) -> Result<Option<Decoder<'a>>> {
        let tag = self.peek_tag()?;
        match tag {
            Tag::OptionNone => {
                self.consume(1)?;
                Ok(None)
            }
            Tag::OptionSome => {
                Ok(Some(self.enter_container(Tag::OptionSome)?))
            }
            _ => Err(Error::InvalidTag(tag as u8))
        }
    }

    /// Decodes a Result.
    ///
    /// Returns `Ok(Decoder)` or `Err(Decoder)` for the respective payloads.
    pub fn result(&mut self) -> Result<std::result::Result<Decoder<'a>, Decoder<'a>>> {
        let tag = self.peek_tag()?;
        match tag {
            Tag::ResultOk => Ok(Ok(self.enter_container(Tag::ResultOk)?)),
            Tag::ResultErr => Ok(Err(self.enter_container(Tag::ResultErr)?)),
            _ => Err(Error::InvalidTag(tag as u8))
        }
    }

    /// Decodes a Variant.
    ///
    /// Returns `(Name, PayloadDecoder)`.
    pub fn variant(&mut self) -> Result<(&'a str, Decoder<'a>)> {
        let mut inner = self.enter_container(Tag::Variant)?;
        let name = inner.str()?;
        Ok((name, inner))
    }
}

/// Iterator for items within a List.
#[derive(Debug)]
pub struct ListIter<'a> {
    dec: Decoder<'a>,
}

impl<'a> ListIter<'a> {
    /// Returns a Decoder for the next item, or `None`.
    pub fn next(&mut self) -> Option<Decoder<'a>> {
        if self.dec.remaining() == 0 {
            return None;
        }
        let mut probe = self.dec.clone();
        if probe.skip().is_err() {
            return None;
        }
        let len = self.dec.remaining() - probe.remaining();
        self.dec.read_slice(len).ok()
    }
}

/// Iterator for Key-Value pairs (Variants) within a Map.
#[derive(Debug)]
pub struct MapIter<'a> {
    dec: Decoder<'a>,
}

impl<'a> MapIter<'a> {
    /// Returns `(Key, ValueDecoder)` for the next item, or `None`.
    pub fn next(&mut self) -> Result<Option<(&'a str, Decoder<'a>)>> {
        if self.dec.remaining() == 0 {
            return Ok(None);
        }
        if self.dec.peek_tag()? != Tag::Variant {
             return Err(Error::InvalidTag(self.dec.peek_tag()? as u8));
        }
        let (name, val) = self.dec.variant()?;
        Ok(Some((name, val)))
    }
}
