//! Unified trait interface for writing Isopack values

use crate::types::Tag;
use crate::types::Result;

/// The unified interface for writing Isopack values.
///
/// This trait is implemented by Encoder, ListEncoder, MapValueEncoder, and other
/// writer types, allowing generic code to write values in any context.
pub trait IsoWriter {
    /// Target type for list contexts
    type ListTarget<'a>: IsoWriter
    where
        Self: 'a;

    /// Target type for map contexts
    type MapTarget<'a>: IsoMapWriter
    where
        Self: 'a;

    /// Target type for array contexts
    type ArrayTarget<'a>: IsoArrayWriter
    where
        Self: 'a;

    // Scalars
    fn bool(&mut self, v: bool) -> Result<()>;
    fn u8(&mut self, v: u8) -> Result<()>;
    fn i8(&mut self, v: i8) -> Result<()>;
    fn u16(&mut self, v: u16) -> Result<()>;
    fn i16(&mut self, v: i16) -> Result<()>;
    fn u32(&mut self, v: u32) -> Result<()>;
    fn i32(&mut self, v: i32) -> Result<()>;
    fn u64(&mut self, v: u64) -> Result<()>;
    fn i64(&mut self, v: i64) -> Result<()>;
    fn f32(&mut self, v: f32) -> Result<()>;
    fn f64(&mut self, v: f64) -> Result<()>;
    fn str(&mut self, v: &str) -> Result<()>;
    fn bytes(&mut self, v: &[u8]) -> Result<()>;
    fn record_raw(&mut self, v: &[u8]) -> Result<()>;

    // ADTs (Algebraic Data Types) - only unit types that don't need payloads
    fn unit(&mut self) -> Result<()>;
    fn option_none(&mut self) -> Result<()>;
    
    // Note: option_some, result_ok, result_err, and variant are not part of the trait
    // because they return ValueEncoder to enforce payload writing at compile-time.
    // They're available as inherent methods on Encoder, ListEncoder, etc.

    // Containers
    fn list(&mut self) -> Result<Self::ListTarget<'_>>;
    fn map(&mut self) -> Result<Self::MapTarget<'_>>;
    fn array(&mut self, tag: Tag, stride: usize) -> Result<Self::ArrayTarget<'_>>;

    /// End the scope. Returns () for all contexts.
    /// The parent context is maintained via stack-based borrowing.
    fn finish(self) -> Result<()>;
}

/// Trait for writing map key-value pairs
pub trait IsoMapWriter {
    /// Target type for writing map values
    type ValueTarget<'a>: IsoWriter
    where
        Self: 'a;

    /// Write a map key and return a writer for the value
    fn key(&mut self, k: &str) -> Result<Self::ValueTarget<'_>>;

    /// Finish writing the map
    fn finish(self) -> Result<()>;
}

/// Trait for writing array elements
pub trait IsoArrayWriter {
    /// Push raw bytes as an array element
    fn push(&mut self, data: &[u8]) -> Result<()>;

    /// Finish writing the array
    fn finish(self) -> Result<()>;
}
