//! Local in-memory transport for testing and thread-to-thread communication

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::mpsc;
use tokio::sync::Mutex;

use crate::Transport;

/// A transport implementation for local, in-memory communication.
/// Uses channels to route messages between two endpoints in the same process.
#[derive(Clone)]
pub struct LocalChannelTransport {
    tx: mpsc::UnboundedSender<(Vec<u8>, mpsc::UnboundedSender<Vec<u8>>)>,
    rx: Arc<Mutex<mpsc::UnboundedReceiver<(Vec<u8>, mpsc::UnboundedSender<Vec<u8>>)>>>,
}

impl LocalChannelTransport {
    /// Create a pair of connected local channel transports.
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

    /// Receive an incoming request and return the payload and response sender.
    pub async fn recv(&self) -> Option<(Vec<u8>, mpsc::UnboundedSender<Vec<u8>>)> {
        self.rx.lock().await.recv().await
    }
}

#[async_trait::async_trait]
impl Transport for LocalChannelTransport {
    async fn call(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let (response_tx, mut response_rx) = mpsc::unbounded_channel();
        self.tx.send((payload.to_vec(), response_tx))?;
        response_rx.recv().await.ok_or_else(|| anyhow::anyhow!("Response channel closed"))
    }
}
