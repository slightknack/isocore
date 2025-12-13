//! # NeoRPC
//!
//! A distinctively strict, schema-driven RPC protocol over Neopack.
//!
//! ## Architecture
//!
//! This library bridges the semantic richness of `wasmtime::component::Val` with the
//! structural rigor of `neopack`. It provides a ledger-like wire format where
//! every RPC interaction is verified against the state machine of the underlying encoder.

mod error;
mod codec;
mod frame;
mod flag;

#[cfg(test)]
mod tests;

pub use error::RpcError;
pub use error::FailureReason;
pub use error::Result;
pub use frame::RpcFrame;
pub use frame::CallFrame;
pub use frame::ReplyFrame;
pub use frame::encode_call;
pub use frame::encode_reply_success;
pub use frame::encode_reply_failure;
pub use frame::decode_frame;
pub use codec::encode_val;
pub use codec::decode_val;
pub use codec::decode_vals;
pub use flag::encode_flags_bitmap;
pub use flag::decode_flags_bitmap;
