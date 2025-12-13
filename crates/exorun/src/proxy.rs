//! # Remote Procedure Call Proxy
//!
//! Marshals Component Model values across RPC boundaries, enforcing protocol
//! correctness and type safety.
//!
//! The proxy is stateless, it operates as a pure function from
//! `(CallEncoder, Transport, FunctionSignature)` to typed results.
//! Sequence generation and transport lifecycle are external concerns.
//!
//! ## Invariants
//!
//! - Results match the signature or an error is returned
//! - Reply sequence numbers must match call sequence numbers
//! - Only Reply frames are accepted; Call frames are protocol violations

use std::sync::Arc;
use std::fmt;

use wasmtime::component::Val;
use neopack::{Encoder, Decoder};
use neorpc::{RpcFrame, CallEncoder, FailureReason};
use crate::transport::{Transport, TransportError};
use crate::ledger::FunctionSignature;

/// Errors during remote invocation.
#[derive(Debug, Clone)]
pub enum ProxyError {
    /// Transport failure (disconnect, timeout).
    Transport(TransportError),
    /// Protocol violation (malformed frames, type mismatch).
    Rpc(neorpc::RpcError),
    /// Remote execution failure (trap, OOM, method not found).
    Remote(FailureReason),
    /// Logic error (sequence mismatch).
    Internal(String),
}

impl fmt::Display for ProxyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(e) => write!(f, "Transport failure: {}", e),
            Self::Rpc(e) => write!(f, "RPC protocol error: {}", e),
            Self::Remote(reason) => write!(f, "Remote instance failure: {:?}", reason),
            Self::Internal(msg) => write!(f, "Internal proxy error: {}", msg),
        }
    }
}

impl std::error::Error for ProxyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Transport(e) => Some(e),
            Self::Rpc(e) => Some(e),
            _ => None,
        }
    }
}

impl From<TransportError> for ProxyError {
    fn from(e: TransportError) -> Self { Self::Transport(e) }
}

impl From<neorpc::RpcError> for ProxyError {
    fn from(e: neorpc::RpcError) -> Self { Self::Rpc(e) }
}

/// Stateless RPC invocation.
pub struct Proxy;

impl Proxy {
    /// Execute a typed remote call.
    ///
    /// Marshals the call, sends it via transport, validates the reply correlation,
    /// and demarshals results according to the signature.
    ///
    /// Returns `Transport` error on network failure, `Rpc` error on protocol
    /// violation, `Remote` error if the remote instance fails, or `Internal`
    /// error on sequence mismatch.
    pub async fn invoke(
        call: CallEncoder<'_>,
        transport: &Arc<dyn Transport>,
        sig: &FunctionSignature,
    ) -> Result<Vec<Val>, ProxyError> {
        // Marshal and send
        let mut enc = Encoder::new();
        call.encode(&mut enc)?;
        let payload = enc.into_bytes().map_err(neorpc::RpcError::from)?;
        let response_bytes = transport.call(&payload).await?;

        // Decode and validate frame type
        let mut dec = Decoder::new(&response_bytes);
        let frame = RpcFrame::decode(&mut dec)?;

        match frame {
            // Protocol violation: we're a client expecting replies
            RpcFrame::Call(_) => {
                Err(ProxyError::Rpc(neorpc::RpcError::ProtocolViolation(
                    "Received Call frame while waiting for Reply".into()
                )))
            }
            // Validate correlation and demarshal results
            RpcFrame::Reply(reply) => {
                if reply.seq != call.seq {
                    return Err(ProxyError::Internal(format!(
                        "Sequence mismatch: sent {}, received {}", call.seq, reply.seq
                    )));
                }

                match reply.status {
                    Ok(val_decoder) => {
                        let results = neorpc::decode_vals(val_decoder, &sig.results)?;
                        Ok(results)
                    }
                    Err(reason) => Err(ProxyError::Remote(reason)),
                }
            }
        }
    }
}
