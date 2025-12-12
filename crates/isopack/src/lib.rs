mod macros;

pub mod types;
pub mod encoder;
pub mod decoder;
pub mod cursor;

pub use types::Result;
pub use types::Error;
pub use types::Tag;

pub use encoder::Encoder;
pub use encoder::ListEncoder;
pub use encoder::MapEncoder;
pub use encoder::ArrayEncoder;
pub use encoder::RecordEncoder;
pub use encoder::RecordBodyEncoder;

pub use decoder::Decoder;
pub use decoder::ListDecoder;
pub use decoder::MapDecoder;
pub use decoder::ArrayDecoder;
pub use decoder::RecordDecoder;
pub use decoder::ValueDecoder;

pub use cursor::Cursor;
pub use cursor::Location;
pub use cursor::StreamBuffer;

#[cfg(test)]
mod tests;
