//! # The Proxy (Airlock Implementation)
//!
//! The Proxy bridges the semantic gap between a high-level `FunctionSignature`
//! and the low-level `Transport`. It manages the lifecycle of a single RPC interaction.
//!
//! ## Responsibilities
//!
//! 1. **Marshalling**: Converts `Val`s to `neopack` bytes using the `Ledger` schema.
//! 2. **Transport IO**: Ships bytes and awaits the reply.
//! 3. **Protocol Enforcement**: Verifies sequence numbers and frame integrity.
//! 4. **Demarshalling**: Reconstructs `Val`s from the reply.
//! 5. **Error Mapping**: Decides if a failure is a Network Error (Trap) or a Remote Error (Trap).

use std::sync::Arc;
use std::fmt;

use wasmtime::component::Val;
use neopack::{Encoder, Decoder};
use neorpc::{RpcFrame, CallEncoder, FailureReason};
use crate::transport::{Transport, TransportError};
use crate::ledger::FunctionSignature;

#[derive(Debug, Clone)]
pub enum ProxyError {
    /// The transport layer failed (e.g. timeout, disconnect).
    Transport(TransportError),
    /// The RPC protocol was violated (serialization, framing, type mismatch).
    Rpc(neorpc::RpcError),
    /// The remote application failed execution (e.g. trapped, OOM).
    Remote(FailureReason),
    /// A critical logic error in the proxy itself (e.g. sequence mismatch).
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

/// The logic core for remote function invocation.
pub struct Proxy;

impl Proxy {
    /// Executes a remote call through the airlock.
    ///
    /// # Arguments
    ///
    /// * `call` - The call encoder with seq, target, method, and args.
    /// * `transport` - The wire carrier.
    /// * `sig` - The type schema from the Ledger.
    ///
    /// # Returns
    ///
    /// * `Ok(Vec<Val>)` - The return values, strictly typed according to `sig.results`.
    /// * `Err(ProxyError)` - A failure to be converted into a Wasm Trap.
    pub async fn invoke(
        call: CallEncoder<'_>,
        transport: &Arc<dyn Transport>,
        sig: &FunctionSignature,
    ) -> Result<Vec<Val>, ProxyError> {
        // 1. Marshalling (The Outbound Airlock)
        let payload = {
            let mut enc = Encoder::new();
            call.encode(&mut enc)?;
            enc.into_bytes().map_err(neorpc::RpcError::from)?
        };

        // 2. The Void (Async Yield)
        let response_bytes = transport.call(&payload).await?;

        // 3. Demarshalling (The Inbound Airlock)
        let mut dec = Decoder::new(&response_bytes);
        let frame = RpcFrame::decode(&mut dec)?;

        match frame {
            RpcFrame::Call(_) => {
                // We are a client; receiving a Call frame is a protocol violation.
                Err(ProxyError::Rpc(neorpc::RpcError::ProtocolViolation(
                    "Received Call frame while waiting for Reply".into()
                )))
            }
            RpcFrame::Reply(reply) => {
                // 4. Correlation Check
                if reply.seq != call.seq {
                    return Err(ProxyError::Internal(format!(
                        "Sequence mismatch: sent {}, received {}", call.seq, reply.seq
                    )));
                }

                // 5. Result Materialization
                match reply.status {
                    Ok(val_decoder) => {
                        // Decode the raw list of values against the Ledger's result schema.
                        let results = neorpc::decode_vals(val_decoder, &sig.results)?;
                        Ok(results)
                    }
                    Err(reason) => {
                        // The remote side trapped or failed cleanly.
                        Err(ProxyError::Remote(reason))
                    }
                }
            }
        }
    }
}
