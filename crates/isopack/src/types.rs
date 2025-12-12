//! Core types for neopack binary format

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    Bool = 0x01,
    S8 = 0x02,
    U8 = 0x03,
    S16 = 0x04,
    U16 = 0x05,
    S32 = 0x06,
    U32 = 0x07,
    S64 = 0x08,
    U64 = 0x09,
    F32 = 0x0A,
    F64 = 0x0B,
    String = 0x10,
    Bytes = 0x11,
    Struct = 0x12,
    List = 0x20,
    Map = 0x21,
    Array = 0x23,
}

impl Tag {
    pub const fn from_u8(b: u8) -> Option<Self> {
        match b {
            0x01 => Some(Tag::Bool),
            0x02 => Some(Tag::S8),
            0x03 => Some(Tag::U8),
            0x04 => Some(Tag::S16),
            0x05 => Some(Tag::U16),
            0x06 => Some(Tag::S32),
            0x07 => Some(Tag::U32),
            0x08 => Some(Tag::S64),
            0x09 => Some(Tag::U64),
            0x0A => Some(Tag::F32),
            0x0B => Some(Tag::F64),
            0x10 => Some(Tag::String),
            0x11 => Some(Tag::Bytes),
            0x12 => Some(Tag::Struct),
            0x20 => Some(Tag::List),
            0x21 => Some(Tag::Map),
            0x23 => Some(Tag::Array),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum Error {
    Pending(usize),
    InvalidTag(u8),
    InvalidUtf8,
    TypeMismatch,
    Malformed,
    BlobTooLarge(usize),
    ContainerFull,
    SeekBeforeBuffer,
    SeekAfterBuffer,
    ScopeOpen,
    PositionFreed,
    OutOfBounds,
}

pub type Result<T> = std::result::Result<T, Error>;
