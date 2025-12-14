//! # RPC Client
//!
//! This module provides the `Client` abstraction for making RPC calls over a transport.
//! It encapsulates the encoding of call frames, transport invocation, and decoding of reply frames.

use std::sync::Arc;

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

#[derive(Debug)]
pub enum Error {
    Transport(transport::Error),
    NeoRpc(neorpc::Error),
    NeoPack(neopack::Error),
    Remote(FailureReason),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(e) => write!(f, "Transport error: {}", e),
            Self::NeoRpc(e) => write!(f, "RPC error: {}", e),
            Error::NeoPack(e) => write!(f, "NeoPack error: {}", e),
            Self::Remote(reason) => write!(f, "Remote failure: {:?}", reason),

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

/// RPC client for making remote calls over a transport.
#[derive(Clone)]
pub struct Client {
    transport: Arc<dyn Transport>,
}

impl Client {
    pub fn new(transport: Arc<dyn Transport>) -> Self {
        Self { transport }
    }

    /// Makes an RPC call and returns the decoded result values.
    ///
    /// # Arguments
    ///
    /// * `call` - The encoder containing the call details (seq, target, method, args).
    /// * `results` - The expected return types, required for decoding the response.
    pub async fn call(
        &self,
        call: CallEncoder<'_>,
        result_types: &[Type],
    ) -> Result<Vec<Val>> {
        // prepare the message
        let mut enc = Encoder::new();
        call.encode(&mut enc)?;
        let payload = enc.into_bytes()?;

        // longingly await a response
        let response = self.transport.call(&payload).await?;
        let mut dec = Decoder::new(&response);
        let frame = RpcFrame::decode(&mut dec)?;

        // check for a reply
        let RpcFrame::Reply(reply) = frame else {
            return Err(Error::NeoRpc(neorpc::Error::ProtocolViolation(
                "Received Call frame while waiting for Reply".into(),
            )));
        };

        // decode the return vals
        let val_decoder = reply.status.map_err(Error::Remote)?;
        let vals = decode_vals(val_decoder, result_types)?;
        Ok(vals)
    }
}
