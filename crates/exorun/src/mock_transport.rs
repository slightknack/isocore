//! Mock transports for testing.
//!
//! These are used internally by the test suite and are not part of the public API.

use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc;

use crate::transport;
use crate::transport::Transport;

/// A duplex channel transport using tokio mpsc channels.
///
/// This mock allows simulating bidirectional communication for testing.
/// Messages sent via send() appear on the peer's recv() and vice versa.
pub struct DuplexChannelTransport {
    tx: mpsc::UnboundedSender<Vec<u8>>,
    rx: Arc<Mutex<mpsc::UnboundedReceiver<Vec<u8>>>>,
}

impl DuplexChannelTransport {
    /// Creates a new transport from separate tx and rx channels.
    pub fn new(
        tx: mpsc::UnboundedSender<Vec<u8>>,
        rx: mpsc::UnboundedReceiver<Vec<u8>>,
    ) -> Self {
        Self {
            tx,
            rx: Arc::new(Mutex::new(rx)),
        }
    }

    /// Creates a pair of transports connected to each other.
    ///
    /// Messages sent on `a` are received by `b` and vice versa.
    pub fn pair() -> (Self, Self) {
        let (tx_a, rx_a) = mpsc::unbounded_channel();
        let (tx_b, rx_b) = mpsc::unbounded_channel();

        let a = Self {
            tx: tx_a,
            rx: Arc::new(Mutex::new(rx_b)),
        };

        let b = Self {
            tx: tx_b,
            rx: Arc::new(Mutex::new(rx_a)),
        };

        (a, b)
    }
}

#[async_trait::async_trait]
impl Transport for DuplexChannelTransport {
    async fn send(&self, payload: &[u8]) -> transport::Result<()> {
        self.tx
            .send(payload.to_vec())
            .map_err(|_| transport::Error::ConnectionLost("Channel closed".into()))
    }

    async fn recv(&self) -> transport::Result<Option<Vec<u8>>> {
        let mut rx = self.rx.lock().await;
        Ok(rx.recv().await)
    }
}

/// A request-response mock transport for simple tests.
///
/// Implements the old call() pattern on top of send/recv for backwards compatibility.
pub struct CallTransport<F>
where
    F: Fn(&[u8]) -> transport::Result<Vec<u8>> + Send + Sync,
{
    handler: F,
}

impl<F> CallTransport<F>
where
    F: Fn(&[u8]) -> transport::Result<Vec<u8>> + Send + Sync,
{
    pub fn new(handler: F) -> Self {
        Self { handler }
    }
}

#[async_trait::async_trait]
impl<F> Transport for CallTransport<F>
where
    F: Fn(&[u8]) -> transport::Result<Vec<u8>> + Send + Sync + 'static,
{
    async fn send(&self, _payload: &[u8]) -> transport::Result<()> {
        // In call-style transport, send is a no-op
        Ok(())
    }

    async fn recv(&self) -> transport::Result<Option<Vec<u8>>> {
        // This shouldn't be called in call-style usage
        Err(transport::Error::Io("CallTransport doesn't support recv".into()))
    }
}

// For backwards compatibility with old tests, we need a way to do synchronous call()
// Let's add a helper that wraps send/recv into a call pattern
pub async fn call_helper(
    transport: &dyn Transport,
    payload: &[u8],
) -> transport::Result<Vec<u8>> {
    transport.send(payload).await?;
    match transport.recv().await? {
        Some(response) => Ok(response),
        None => Err(transport::Error::ConnectionLost("Stream closed".into())),
    }
}
