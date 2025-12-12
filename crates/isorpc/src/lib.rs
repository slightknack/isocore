// crates/isorpc/src/lib.rs
//! RPC encoding for WebAssembly component model values using isopack.

mod decode;
mod encode;
mod message;
mod types;

#[cfg(test)]
mod tests;

pub use crate::types::Result;
pub use crate::types::Error;
pub use crate::types::MessageHeader;

pub use crate::message::encode_call;
pub use crate::message::encode_response_ok;
pub use crate::message::encode_response_err;
pub use crate::message::decode_header;

pub use crate::encode::encode_value;
pub use crate::encode::encode_vals;

pub use crate::decode::convert_val;
pub use crate::decode::decode_vals;
