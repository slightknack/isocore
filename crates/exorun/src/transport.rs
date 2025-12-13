// --- crates/exorun/src/transport.rs ---
//! # Transport Abstraction
//!
//! A minimal, async interface for moving bytes between runtimes.
//!
//! ## Philosophy
//!
//! - **Byte-Oriented**: The Transport knows nothing about RPC frames, Val, or Types.
//!   It moves opaque buffers.
//! - **Request-Response**: The fundamental interaction model is "send bytes, await bytes".
//!   One-way messages or streams are built on top of this, not defined here.

use std::fmt;

/// Errors that occur at the network/transport layer.
#[derive(Debug, Clone)]
pub enum TransportError {
    /// The peer is unreachable or the connection was dropped.
    ConnectionLost(String),
    /// The operation timed out before a response was received.
    Timeout,
    /// The remote peer rejected the payload size.
    PayloadTooLarge,
    /// Generic I/O error or internal transport failure.
    Io(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConnectionLost(msg) => write!(f, "Connection lost: {}", msg),
            Self::Timeout => write!(f, "Request timed out"),
            Self::PayloadTooLarge => write!(f, "Payload too large for transport"),
            Self::Io(msg) => write!(f, "I/O error: {}", msg),
        }
    }
}

impl std::error::Error for TransportError {}

pub type Result<T> = std::result::Result<T, TransportError>;

/// A mechanism to send a byte buffer and receive a reply.
///
/// This trait is designed to be object-safe (`Arc<dyn Transport>`).
#[async_trait::async_trait]
pub trait Transport: Send + Sync + 'static {
    /// Sends a payload and waits for a response.
    ///
    /// This is a blocking operation from the perspective of the async task.
    ///
    /// # invariants
    /// - Must return `Ok(vec)` with the raw reply bytes on success.
    /// - Must return `Err` if the network fails.
    /// - Should not interpret the payload content (e.g. no JSON parsing).
    async fn call(&self, payload: &[u8]) -> Result<Vec<u8>>;
}
