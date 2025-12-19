//! # Message-based transport trait for connecting peers
//!
//! A minimal, async interface for moving bytes between runtimes.
//! Each transport is held by exactly one Peer, who is responsible for
//! Packaging messages, assigning sequence numbers, and
//! pairing responses to corresponding requests.
//!
//! ## Philosophy
//!
//! - **Byte-Oriented**: The Transport knows nothing about RPC frames, Val, or Types.
//!   It moves opaque buffers.
//! - **Message-Passing**: The fundamental interaction model is asynchronous message passing.
//!   Request-response, streams, and other patterns are built on top using sequence numbers.

use std::fmt;

/// Errors that occur at the network/transport layer.
#[derive(Debug, Clone)]
pub enum Error {
    /// The peer is unreachable or the connection was dropped.
    ConnectionLost(String),
    /// The operation timed out before a response was received.
    Timeout,
    /// The remote peer rejected the payload size.
    PayloadTooLarge,
    /// Generic I/O error or internal transport failure.
    Io(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConnectionLost(msg) => write!(f, "connection lost: {}", msg),
            Self::Timeout => write!(f, "request timed out"),
            Self::PayloadTooLarge => write!(f, "payload too large for transport"),
            Self::Io(msg) => write!(f, "i/o error: {}", msg),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// A mechanism for asynchronous message passing between runtimes.
///
/// This trait is designed to be object-safe (`Arc<dyn Transport>`).
/// It provides low-level message send/receive primitives. Higher-level
/// patterns like request-response are implemented in the Client.
#[async_trait::async_trait]
pub trait Transport: Send + Sync + 'static {
    /// Queues a raw message for transmission.
    ///
    /// This should handle framing (e.g., length-prefixing) appropriate for the
    /// underlying stream. The method returns immediately after queuing.
    ///
    /// # Invariants
    /// - Must not block on network I/O
    /// - Should return `Err` only on permanent failures
    async fn send(&self, payload: &[u8]) -> Result<()>;

    /// Awaits the next complete message from the peer.
    ///
    /// This method blocks until a message is available or the stream is closed.
    ///
    /// # Returns
    /// - `Ok(Some(bytes))` - A complete message was received
    /// - `Ok(None)` - The stream is closed (EOF)
    /// - `Err(_)` - A transport error occurred
    ///
    /// # Invariants
    /// - Messages are returned in order
    /// - Each message is complete (no partial reads)
    async fn recv(&self) -> Result<Option<Vec<u8>>>;
}
