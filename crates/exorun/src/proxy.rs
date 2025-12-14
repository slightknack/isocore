//! # Remote Procedure Call Proxy
//!
//! Marshals Component Model values across RPC boundaries, enforcing protocol
//! correctness and type safety.
//!
//! The proxy is stateless, it operates as a pure function from
//! `(payload, Transport, FunctionSignature)` to typed results.
//! Encoding and transport lifecycle are external concerns.
//!
//! ## Invariants
//!
//! - Results match the signature or an error is returned
//! - Only Reply frames are accepted; Call frames are protocol violations

use std::sync::Arc;
use std::fmt;

use neopack::Decoder;
use neorpc::FailureReason;
use neorpc::RpcFrame;
use wasmtime::component::Val;

use crate::ledger::FunctionSignature;
use crate::transport::Transport;
use crate::transport::TransportError;

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
    /// Takes pre-encoded call payload, sends it via transport, validates the reply,
    /// and demarshals results according to the signature.
    ///
    /// Returns `Transport` error on network failure, `Rpc` error on protocol
    /// violation, or `Remote` error if the remote instance fails.
    pub async fn invoke(
        payload: &[u8],
        transport: &Arc<dyn Transport>,
        sig: &FunctionSignature,
    ) -> Result<Vec<Val>, ProxyError> {
        let response_bytes = transport.call(payload).await?;

        let mut dec = Decoder::new(&response_bytes);
        let frame = RpcFrame::decode(&mut dec)?;

        match frame {
            RpcFrame::Call(_) => {
                Err(ProxyError::Rpc(neorpc::RpcError::ProtocolViolation(
                    "Received Call frame while waiting for Reply".into()
                )))
            }
            RpcFrame::Reply(reply) => {
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
