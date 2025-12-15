//! # RPC Peer with Async Pump
//!
//! This module provides the `Peer` abstraction for making RPC calls over a transport.
//! It uses an async pump task to demultiplex incoming responses and correlate them
//! with pending requests via sequence numbers.

use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;

use dashmap::DashMap;
use tokio::sync::oneshot;

use neopack::Decoder;
use neopack::Encoder;
use neorpc::CallEncoder;
use neorpc::FailureReason;
use neorpc::RpcFrame;
use neorpc::decode_vals;
use wasmtime::component::Type;
use wasmtime::component::Val;

use crate::transport::Transport;
use crate::transport;

#[derive(Debug, Clone)]
pub enum Error {
    Transport(transport::Error),
    NeoRpc(neorpc::Error),
    NeoPack(neopack::Error),
    Remote(FailureReason),
    Timeout,
    ChannelClosed,
    SequenceMismatch { expected: u64, received: u64 },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(e) => write!(f, "Transport error: {}", e),
            Self::NeoRpc(e) => write!(f, "RPC error: {}", e),
            Self::NeoPack(e) => write!(f, "NeoPack error: {}", e),
            Self::Remote(reason) => write!(f, "Remote failure: {:?}", reason),
            Self::Timeout => write!(f, "Request timed out"),
            Self::ChannelClosed => write!(f, "Response channel closed"),
            Self::SequenceMismatch { expected, received } => {
                write!(f, "Sequence mismatch: expected {}, received {}", expected, received)
            }
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

/// Response data correlating to a request.
struct PendingResponse {
    result_types: Vec<Type>,
    tx: oneshot::Sender<Result<Vec<Val>>>,
}

/// RPC peer with async message pump for concurrent requests.
///
/// The peer spawns a background task that continuously reads from the transport
/// and routes responses to the appropriate pending request based on sequence number.
///
/// Each Peer owns its transport exclusively, ensuring sequence numbers are scoped
/// to a single transport. Peers can be wrapped in Arc for sharing across threads.
pub struct Peer {
    peer_name: String,
    transport: Arc<dyn Transport>,
    pending: Arc<DashMap<u64, PendingResponse>>,
    seq_gen: Arc<AtomicU64>,
}

impl Peer {
    /// Creates a new peer and spawns the background pump task.
    /// Takes ownership of the transport and converts it to Arc internally.
    /// The peer_name is used for logging and diagnostics.
    pub fn new(peer_name: impl Into<String>, transport: Box<dyn Transport>) -> Self {
        let peer_name = peer_name.into();
        let transport: Arc<dyn Transport> = Arc::from(transport);
        let pending = Arc::new(DashMap::new());

        // Spawn the pump task
        let pump_transport = transport.clone();
        let pump_pending = pending.clone();

        tokio::spawn(async move {
            let error = loop {
                match pump_transport.recv().await {
                    Ok(Some(msg)) => {
                        if let Err(e) = Self::handle_message(&msg, &pump_pending) {
                            eprintln!("Error handling message in pump: {}", e);
                            break e;
                        }
                    }
                    Ok(None) => {
                        // Stream closed
                        break Error::Transport(transport::Error::ConnectionLost("Stream closed".into()));
                    }
                    Err(e) => {
                        eprintln!("Transport error in pump: {}", e);
                        break Error::Transport(e);
                    }
                }
            };

            // Notify all pending requests with the error
            Self::notify_all_pending(&pump_pending, error);
        });

        Self {
            peer_name,
            transport,
            pending,
            seq_gen: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Returns the peer name for this client.
    pub fn peer_name(&self) -> &str {
        &self.peer_name
    }

    /// Notify all pending requests with the given error.
    fn notify_all_pending(pending: &DashMap<u64, PendingResponse>, error: Error) {
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
                // decode values
                let vals = decode_vals(val_decoder, &pending_resp.result_types)
                    .map_err(Error::from)?;

                // make sure we got all of them
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

    /// Prepares an RPC call by incrementing the sequence number and registering
    /// a pending response. Returns the sequence number and a future that will
    /// resolve when the response arrives.
    ///
    /// This method is designed to be used with a closure that encodes the request
    /// payload, avoiding unnecessary copying of argument values.
    pub fn prepare_call(
        &self,
        result_types: Vec<Type>,
    ) -> (u64, oneshot::Receiver<Result<Vec<Val>>>) {
        let seq = self.seq_gen.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();

        self.pending.insert(seq, PendingResponse {
            result_types,
            tx,
        });

        (seq, rx)
    }

    /// Sends an encoded RPC frame and awaits the response.
    ///
    /// This is a lower-level method that allows the caller to encode the frame
    /// themselves, avoiding intermediate allocations.
    pub async fn send_and_await(
        &self,
        seq: u64,
        payload: Vec<u8>,
        rx: oneshot::Receiver<Result<Vec<Val>>>,
    ) -> Result<Vec<Val>> {
        // Send the request
        if let Err(e) = self.transport.send(&payload).await {
            self.pending.remove(&seq);
            return Err(e.into());
        }

        // Await response with timeout
        match tokio::time::timeout(Duration::from_secs(30), rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => {
                self.pending.remove(&seq);
                Err(Error::ChannelClosed)
            }
            Err(_) => {
                self.pending.remove(&seq);
                Err(Error::Timeout)
            }
        }
    }

    /// This method encodes the request, sends it, and awaits the response
    /// with a timeout. The response is correlated via sequence number.
    pub async fn call(
        &self,
        target: &str,
        method: &str,
        args: &[Val],
        result_types: Vec<Type>,
    ) -> Result<Vec<Val>> {
        let (seq, rx) = self.prepare_call(result_types);

        // Step 1: Encode arguments via codec (produces Vec<u8>)
        // Step 2: Encode frame via framing (injects args_bytes)
        let args_bytes = neorpc::encode_vals_to_bytes(args)?;
        let mut enc = Encoder::new();
        CallEncoder::new(seq, target, method, &args_bytes).encode(&mut enc)?;
        let payload = enc.into_bytes()?;

        self.send_and_await(seq, payload, rx).await
    }
}
