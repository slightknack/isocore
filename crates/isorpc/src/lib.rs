//! RPC encoding for WebAssembly component model values using isopack.
//!
//! This crate provides:
//! - Generic Val serialization via the IsoWriter trait
//! - Val deserialization with Type information
//! - RPC call/response encoding using the "Everything is a List" protocol

mod decode;
mod encode;
mod rpc;
mod types;

#[cfg(test)]
mod tests;

// Re-export main RPC functions
pub use rpc::encode_call;
pub use rpc::encode_response_err;
pub use rpc::encode_response_ok;

// Re-export RPC types
pub use types::RpcCall;
pub use types::RpcResponse;

// Re-export Val encoding/decoding
pub use decode::decode_val;
pub use encode::encode_val;
