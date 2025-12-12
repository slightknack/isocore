//! Local in-memory transport for testing and thread-to-thread communication

use crate::Transport;
use anyhow::Result;

/// A transport implementation for local, in-memory communication.
/// Uses channels to route messages between two endpoints in the same process.
#[derive(Clone)]
pub struct LocalChannelTransport {
    _marker: std::marker::PhantomData<()>,
}

impl LocalChannelTransport {
    /// Create a pair of connected local channel transports.
    pub fn pair() -> (Self, Self) {
        // Implementation note: This should use tokio::sync::mpsc channels
        // to create a bidirectional communication channel between two
        // endpoints in the same process.
        let a = Self {
            _marker: std::marker::PhantomData,
        };
        let b = Self {
            _marker: std::marker::PhantomData,
        };
        (a, b)
    }
}

#[async_trait::async_trait]
impl Transport for LocalChannelTransport {
    async fn call(&self, _payload: &[u8]) -> Result<Vec<u8>> {
        todo!("Implement local channel transport call")
    }
}
