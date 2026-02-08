//! # Peer components on other machines called via RPC
//!
//! This module provides the `Peer` abstraction for making RPC calls over a Transport.
//! It uses an async pump task to demultiplex incoming responses and correlate them
//! with pending requests via sequence numbers.
//!
//! ## Features
//!
//! - **Lifecycle Management**: Peers track their connection state (Connected, Disconnected, Shutdown)
//! - **Reconnection**: Transports can be replaced without losing peer identity
//! - **Configurable Timeouts**: Per-peer and per-call timeout configuration
//! - **Backpressure**: Optional limit on pending requests
//!
//! ## Example
//!
//! ```ignore
//! let transport = QuicTransport::connect("peer.example.com:4433").await?;
//! let config = PeerConfig {
//!     call_timeout: Duration::from_secs(10),
//!     max_pending: 100,
//! };
//! let peer = Peer::new("alice", transport, config);
//!
//! // Make calls
//! let result = peer.call("service", "method", &args, result_types).await?;
//!
//! // On disconnect, reconnect with new transport
//! let ws = WebSocketTransport::connect("wss://peer.example.com/rpc").await?;
//! peer.reconnect(ws).await?;
//! ```

#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::time::Duration;

use dashmap::DashMap;
use tokio::sync::{oneshot, Notify};
use tokio::task::JoinHandle;

use neopack::Decoder;
use neopack::Encoder;
use neorpc::CallEncoder;
use neorpc::FailureReason;
use neorpc::RpcFrame;
use neorpc::decode_vals;
use wasmtime::component::Type;
use wasmtime::component::Val;

use crate::runtime::PeerId;
use crate::transport::Transport;
use crate::transport;

// =============================================================================
// Error Types
// =============================================================================

#[derive(Debug, Clone)]
pub enum Error {
    /// Transport-level error.
    Transport(transport::Error),
    /// RPC protocol error.
    NeoRpc(neorpc::Error),
    /// Serialization error.
    NeoPack(neopack::Error),
    /// Remote peer returned a failure.
    Remote(FailureReason),
    /// Request timed out waiting for response.
    Timeout,
    /// Response channel was closed unexpectedly.
    ChannelClosed,
    /// Response sequence number didn't match request.
    SequenceMismatch { expected: u64, received: u64 },
    /// Peer is disconnected and cannot process calls.
    Disconnected,
    /// Peer has been shut down and cannot be used.
    Shutdown,
    /// Too many pending requests (backpressure).
    TooManyPendingRequests { limit: usize },
    /// Cannot reconnect because peer is already connected.
    AlreadyConnected,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(e) => write!(f, "transport error: {}", e),
            Self::NeoRpc(e) => write!(f, "rpc error: {}", e),
            Self::NeoPack(e) => write!(f, "neopack error: {}", e),
            Self::Remote(reason) => write!(f, "remote failure: {:?}", reason),
            Self::Timeout => write!(f, "request timed out"),
            Self::ChannelClosed => write!(f, "response channel closed"),
            Self::SequenceMismatch { expected, received } => {
                write!(f, "sequence mismatch: expected {}, received {}", expected, received)
            }
            Self::Disconnected => write!(f, "peer is disconnected"),
            Self::Shutdown => write!(f, "peer has been shut down"),
            Self::TooManyPendingRequests { limit } => {
                write!(f, "too many pending requests (limit: {})", limit)
            }
            Self::AlreadyConnected => write!(f, "peer is already connected"),
        }
    }
}

impl std::error::Error for Error {}

impl From<transport::Error> for Error {
    fn from(e: transport::Error) -> Self {
        Self::Transport(e)
    }
}

impl From<neorpc::Error> for Error {
    fn from(e: neorpc::Error) -> Self {
        Self::NeoRpc(e)
    }
}

impl From<neopack::Error> for Error {
    fn from(e: neopack::Error) -> Self {
        Self::NeoPack(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

// =============================================================================
// Configuration
// =============================================================================

/// Configuration for peer behavior.
#[derive(Clone, Debug)]
pub struct PeerConfig {
    /// Default timeout for RPC calls.
    pub call_timeout: Duration,
    /// Maximum number of pending requests (0 = unlimited).
    pub max_pending: usize,
}

impl Default for PeerConfig {
    fn default() -> Self {
        Self {
            call_timeout: Duration::from_secs(30),
            max_pending: 0, // unlimited
        }
    }
}

// =============================================================================
// Peer State
// =============================================================================

/// Observable state of a peer connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PeerState {
    /// Transport is connected and pump is running.
    Connected = 0,
    /// Transport disconnected, awaiting reconnection.
    Disconnected = 1,
    /// Peer has been shut down and cannot be used.
    Shutdown = 2,
}

impl PeerState {
    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Connected,
            1 => Self::Disconnected,
            _ => Self::Shutdown,
        }
    }
}

// =============================================================================
// Internal Types
// =============================================================================

/// Response data correlating to a request.
struct PendingResponse {
    result_types: Vec<Type>,
    tx: oneshot::Sender<Result<Vec<Val>>>,
}

/// A handle to the resources needed to bind a remote target.
/// Uses a logical PeerId that will be resolved to a Peer at call time
/// via the Runtime in ExorunCtx.
#[derive(Clone)]
pub struct PeerInstance {
    pub peer_id: PeerId,
    pub target_id: String,
}

/// Shared state between Peer and pump task.
struct PeerInner {
    peer_name: String,
    config: PeerConfig,
    state: AtomicU8,
    pending: DashMap<u64, PendingResponse>,
    seq_gen: AtomicU64,
    shutdown_notify: Notify,
}

// =============================================================================
// Peer
// =============================================================================

/// RPC peer with async message pump for concurrent requests.
///
/// The peer spawns a background task that continuously reads from the transport
/// and routes responses to the appropriate pending request based on sequence number.
///
/// ## Lifecycle
///
/// A peer transitions through the following states:
/// - `Connected`: Transport is active, calls are processed normally
/// - `Disconnected`: Transport failed, calls return `Error::Disconnected`
/// - `Shutdown`: Peer is terminated, calls return `Error::Shutdown`
///
/// ## Reconnection
///
/// When a peer becomes disconnected, you can call `reconnect()` with a new
/// transport. The peer identity (and PeerId) is preserved, allowing existing
/// bindings to continue working.
pub struct Peer {
    inner: Arc<PeerInner>,
    pump_handle: tokio::sync::Mutex<Option<JoinHandle<()>>>,
    transport: tokio::sync::Mutex<Option<Arc<dyn Transport>>>,
}

impl Peer {
    /// Creates a new peer and spawns the background pump task.
    ///
    /// Takes ownership of the transport and spawns a background task to
    /// demultiplex incoming responses.
    pub fn new(
        peer_name: impl Into<String>,
        transport: Box<dyn Transport>,
        config: PeerConfig,
    ) -> Self {
        let transport: Arc<dyn Transport> = Arc::from(transport);
        
        let inner = Arc::new(PeerInner {
            peer_name: peer_name.into(),
            config,
            state: AtomicU8::new(PeerState::Connected as u8),
            pending: DashMap::new(),
            seq_gen: AtomicU64::new(1),
            shutdown_notify: Notify::new(),
        });

        let pump_handle = Self::spawn_pump(inner.clone(), transport.clone());

        Self {
            inner,
            pump_handle: tokio::sync::Mutex::new(Some(pump_handle)),
            transport: tokio::sync::Mutex::new(Some(transport)),
        }
    }

    /// Returns the peer name for logging and diagnostics.
    pub fn peer_name(&self) -> &str {
        &self.inner.peer_name
    }

    /// Returns the current connection state.
    pub fn state(&self) -> PeerState {
        PeerState::from_u8(self.inner.state.load(Ordering::SeqCst))
    }

    /// Replaces the transport, restarting the pump.
    ///
    /// This allows reconnecting to a peer via a different protocol or address
    /// while preserving the peer identity. The peer must be in the `Disconnected`
    /// state; reconnecting while connected or shutdown returns an error.
    ///
    /// Pending requests from before the disconnect will have already been
    /// notified with errors. New requests after reconnection will use the
    /// new transport.
    pub async fn reconnect(&self, transport: Box<dyn Transport>) -> Result<()> {
        let current_state = self.state();
        
        if current_state == PeerState::Shutdown {
            return Err(Error::Shutdown);
        }
        
        if current_state == PeerState::Connected {
            return Err(Error::AlreadyConnected);
        }

        let transport: Arc<dyn Transport> = Arc::from(transport);
        
        // Cancel old pump if still somehow running
        {
            let mut handle_guard = self.pump_handle.lock().await;
            if let Some(handle) = handle_guard.take() {
                handle.abort();
            }
        }

        // Update state to connected
        self.inner.state.store(PeerState::Connected as u8, Ordering::SeqCst);

        // Spawn new pump
        let new_handle = Self::spawn_pump(self.inner.clone(), transport.clone());
        
        // Store new transport and handle
        *self.transport.lock().await = Some(transport);
        *self.pump_handle.lock().await = Some(new_handle);

        Ok(())
    }

    /// Gracefully shuts down the peer.
    ///
    /// This notifies all pending requests with `Error::Shutdown` and stops
    /// the pump task. The peer cannot be used after shutdown; all subsequent
    /// calls will return `Error::Shutdown`.
    ///
    /// This method is idempotent; calling it multiple times is safe.
    pub async fn shutdown(&self) {
        // Set state to shutdown
        self.inner.state.store(PeerState::Shutdown as u8, Ordering::SeqCst);
        
        // Signal pump to stop
        self.inner.shutdown_notify.notify_waiters();
        
        // Cancel pump task
        {
            let mut handle_guard = self.pump_handle.lock().await;
            if let Some(handle) = handle_guard.take() {
                handle.abort();
                let _ = handle.await; // Ignore join error from abort
            }
        }
        
        // Drop transport
        *self.transport.lock().await = None;
        
        // Notify all pending requests
        Self::notify_all_pending(&self.inner.pending, Error::Shutdown);
    }

    /// Makes an RPC call with the configured default timeout.
    pub async fn call(
        &self,
        target: &str,
        method: &str,
        args: &[Val],
        result_types: Vec<Type>,
    ) -> Result<Vec<Val>> {
        self.call_with_timeout(target, method, args, result_types, self.inner.config.call_timeout).await
    }

    /// Makes an RPC call with a custom timeout.
    pub async fn call_with_timeout(
        &self,
        target: &str,
        method: &str,
        args: &[Val],
        result_types: Vec<Type>,
        timeout: Duration,
    ) -> Result<Vec<Val>> {
        // Check state before doing any work
        let state = self.state();
        if state == PeerState::Shutdown {
            return Err(Error::Shutdown);
        }
        if state == PeerState::Disconnected {
            return Err(Error::Disconnected);
        }

        // Check backpressure limit
        let max_pending = self.inner.config.max_pending;
        if max_pending > 0 && self.inner.pending.len() >= max_pending {
            return Err(Error::TooManyPendingRequests { limit: max_pending });
        }

        let (seq, rx) = self.prepare_call(result_types);

        // Encode the call
        let args_bytes = neorpc::encode_vals_to_bytes(args)?;
        let mut enc = Encoder::new();
        CallEncoder::new(seq, target, method, &args_bytes).encode(&mut enc)?;
        let payload = enc.into_bytes()?;

        // Get transport (might be None if disconnected between check and here)
        let transport = {
            let guard = self.transport.lock().await;
            match &*guard {
                Some(t) => t.clone(),
                None => {
                    self.inner.pending.remove(&seq);
                    return Err(Error::Disconnected);
                }
            }
        };

        // Send the request
        if let Err(e) = transport.send(&payload).await {
            self.inner.pending.remove(&seq);
            return Err(e.into());
        }

        // Await response with timeout
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => {
                self.inner.pending.remove(&seq);
                Err(Error::ChannelClosed)
            }
            Err(_) => {
                self.inner.pending.remove(&seq);
                Err(Error::Timeout)
            }
        }
    }

    /// Prepares an RPC call by incrementing the sequence number and registering
    /// a pending response.
    ///
    /// This is a lower-level API for advanced use cases where you want to
    /// encode the call yourself.
    pub fn prepare_call(
        &self,
        result_types: Vec<Type>,
    ) -> (u64, oneshot::Receiver<Result<Vec<Val>>>) {
        let seq = self.inner.seq_gen.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();

        self.inner.pending.insert(seq, PendingResponse {
            result_types,
            tx,
        });

        (seq, rx)
    }

    /// Sends an encoded RPC frame and awaits the response.
    ///
    /// This is a lower-level API that allows the caller to encode the frame
    /// themselves, avoiding intermediate allocations.
    pub async fn send_and_await(
        &self,
        seq: u64,
        payload: Vec<u8>,
        rx: oneshot::Receiver<Result<Vec<Val>>>,
    ) -> Result<Vec<Val>> {
        // Check state
        let state = self.state();
        if state == PeerState::Shutdown {
            self.inner.pending.remove(&seq);
            return Err(Error::Shutdown);
        }
        if state == PeerState::Disconnected {
            self.inner.pending.remove(&seq);
            return Err(Error::Disconnected);
        }

        // Get transport
        let transport = {
            let guard = self.transport.lock().await;
            match &*guard {
                Some(t) => t.clone(),
                None => {
                    self.inner.pending.remove(&seq);
                    return Err(Error::Disconnected);
                }
            }
        };

        // Send the request
        if let Err(e) = transport.send(&payload).await {
            self.inner.pending.remove(&seq);
            return Err(e.into());
        }

        // Await response with configured timeout
        let timeout = self.inner.config.call_timeout;
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => {
                self.inner.pending.remove(&seq);
                Err(Error::ChannelClosed)
            }
            Err(_) => {
                self.inner.pending.remove(&seq);
                Err(Error::Timeout)
            }
        }
    }

    // =========================================================================
    // Private Helpers
    // =========================================================================

    /// Spawns the pump task that reads from the transport.
    fn spawn_pump(inner: Arc<PeerInner>, transport: Arc<dyn Transport>) -> JoinHandle<()> {
        tokio::spawn(async move {
            let error = loop {
                tokio::select! {
                    // Check for shutdown signal
                    _ = inner.shutdown_notify.notified() => {
                        break Error::Shutdown;
                    }
                    // Read from transport
                    result = transport.recv() => {
                        match result {
                            Ok(Some(msg)) => {
                                if let Err(e) = Self::handle_message(&msg, &inner.pending) {
                                    eprintln!("[{}] Error handling message in pump: {}", inner.peer_name, e);
                                    break e;
                                }
                            }
                            Ok(None) => {
                                // Stream closed (EOF)
                                break Error::Transport(transport::Error::ConnectionLost("Stream closed".into()));
                            }
                            Err(e) => {
                                eprintln!("[{}] Transport error in pump: {}", inner.peer_name, e);
                                break Error::Transport(e);
                            }
                        }
                    }
                }
            };

            // Update state to disconnected (unless already shutdown)
            let current = inner.state.load(Ordering::SeqCst);
            if current != PeerState::Shutdown as u8 {
                inner.state.store(PeerState::Disconnected as u8, Ordering::SeqCst);
            }

            // Notify all pending requests with the error
            Self::notify_all_pending(&inner.pending, error);
        })
    }

    /// Notify all pending requests with the given error.
    fn notify_all_pending(pending: &DashMap<u64, PendingResponse>, error: Error) {
        // Collect keys first to avoid holding iterator across await points
        let keys: Vec<u64> = pending.iter().map(|e| *e.key()).collect();
        for key in keys {
            if let Some((_, pending_resp)) = pending.remove(&key) {
                let _ = pending_resp.tx.send(Err(error.clone()));
            }
        }
    }

    /// Handle an incoming message from the transport.
    fn handle_message(msg: &[u8], pending: &DashMap<u64, PendingResponse>) -> Result<()> {
        let mut dec = Decoder::new(msg);
        let frame = RpcFrame::decode(&mut dec)?;

        let RpcFrame::Reply(reply) = frame else {
            return Err(Error::NeoRpc(neorpc::Error::ProtocolViolation(
                "Pump received Call frame instead of Reply".into(),
            )));
        };

        let seq = reply.seq;

        // Find and remove the pending request
        let Some((_, pending_resp)) = pending.remove(&seq) else {
            // No pending request for this sequence - might be a duplicate or very late response
            return Ok(());
        };

        // Decode the result
        let result = match reply.status {
            Ok(val_decoder) => {
                let vals = decode_vals(val_decoder, &pending_resp.result_types)
                    .map_err(Error::from)?;

                let expected = pending_resp.result_types.len();
                let actual = vals.len();
                if expected != actual {
                    let message = format!("Result count mismatch: expected {}, got {}", expected, actual);
                    let error = neorpc::Error::ProtocolViolation(message);
                    return Err(Error::NeoRpc(error));
                }

                Ok(vals)
            }
            Err(reason) => Err(Error::Remote(reason)),
        };

        // Send result to waiting caller (ignore if receiver dropped)
        let _ = pending_resp.tx.send(result);

        Ok(())
    }
}
