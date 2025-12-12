//! RPC types and serialization for isorun.
//!
//! RPC protocol uses the "Everything is a List" encoding from isorpc:
//! - RpcCall: [seq, func_name, [args...]]
//! - RpcResponse: [seq, Result<val, error>]

use anyhow::Context;
use anyhow::Result;
use isopack::Decoder;
use isopack::Encoder;
use isopack::ValueDecoder;
use isorpc::serialize_val;
use wasmtime::component::Val;

/// RPC call structure
#[derive(Debug, Clone)]
pub struct RpcCall {
    /// Sequence number for matching requests and responses
    pub seq: u64,
    /// Function name to call
    pub function: String,
    /// Function arguments
    pub args: Vec<Val>,
}

impl RpcCall {
    /// Create a new RPC call
    pub fn new(seq: u64, function: String, args: Vec<Val>) -> Self {
        Self { seq, function, args }
    }

    /// Encode the RPC call to bytes
    pub fn encode(&self) -> Result<Vec<u8>> {
        isorpc::encode_call(self.seq, &self.function, &self.args)
            .context("Failed to encode RPC call")
    }

    /// Decode an RPC call from bytes
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut dec = Decoder::new(bytes);
        let mut list = dec.list().context("Expected list for RPC call")?;

        // Sequence number
        let seq = list
            .next()
            .context("Missing sequence number")??
            .as_u64()
            .context("Sequence number must be u64")?;

        // Function name
        let function = list
            .next()
            .context("Missing function name")??
            .as_str()
            .context("Function name must be string")?
            .to_string();

        // Arguments list
        let mut args_list = match list.next().context("Missing arguments list")?? {
            ValueDecoder::List(l) => l,
            _ => anyhow::bail!("Arguments must be a list"),
        };

        let mut args = Vec::new();
        while let Some(value_dec) = args_list.next()? {
            // TODO: Implement Val deserialization
            // For now, we'll need to add a decode_val function
            anyhow::bail!("Val deserialization not yet implemented");
        }

        Ok(Self { seq, function, args })
    }
}

/// RPC response structure
#[derive(Debug, Clone)]
pub struct RpcResponse {
    /// Sequence number matching the request
    pub seq: u64,
    /// Response result
    pub result: Result<Val, String>,
}

impl RpcResponse {
    /// Create a successful RPC response
    pub fn ok(seq: u64, value: Val) -> Self {
        Self {
            seq,
            result: Ok(value),
        }
    }

    /// Create an error RPC response
    pub fn err(seq: u64, error: String) -> Self {
        Self {
            seq,
            result: Err(error),
        }
    }

    /// Encode the RPC response to bytes
    pub fn encode(&self) -> Result<Vec<u8>> {
        match &self.result {
            Ok(val) => isorpc::encode_response_ok(self.seq, val)
                .context("Failed to encode RPC response"),
            Err(err) => isorpc::encode_response_err(self.seq, err)
                .context("Failed to encode RPC error"),
        }
    }

    /// Decode an RPC response from bytes
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut dec = Decoder::new(bytes);
        let mut list = dec.list().context("Expected list for RPC response")?;

        // Sequence number
        let seq = list
            .next()
            .context("Missing sequence number")??
            .as_u64()
            .context("Sequence number must be u64")?;

        // Result<Val, String>
        let result = match list.next().context("Missing result")?? {
            ValueDecoder::ResultOk => {
                // TODO: Implement Val deserialization
                anyhow::bail!("Val deserialization not yet implemented");
            }
            ValueDecoder::ResultErr => {
                let error = list
                    .next()
                    .context("Missing error message")??
                    .as_str()
                    .context("Error must be string")?
                    .to_string();
                Err(error)
            }
            _ => anyhow::bail!("Expected Result type"),
        };

        Ok(Self { seq, result })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rpc_call_encode() -> Result<()> {
        let call = RpcCall::new(
            123,
            "my_func".to_string(),
            vec![Val::U32(42), Val::String("hello".into())],
        );

        let bytes = call.encode()?;

        // Verify basic structure
        let mut dec = Decoder::new(&bytes);
        let mut list = dec.list()?;

        assert_eq!(list.next()?.unwrap().as_u64()?, 123);
        assert_eq!(list.next()?.unwrap().as_str()?, "my_func");

        Ok(())
    }

    #[test]
    fn test_rpc_response_ok_encode() -> Result<()> {
        let response = RpcResponse::ok(456, Val::U64(9999));
        let bytes = response.encode()?;

        // Verify basic structure
        let mut dec = Decoder::new(&bytes);
        let mut list = dec.list()?;

        assert_eq!(list.next()?.unwrap().as_u64()?, 456);

        match list.next()?.unwrap() {
            ValueDecoder::ResultOk => {
                assert_eq!(list.next()?.unwrap().as_u64()?, 9999);
            }
            _ => panic!("Expected ResultOk"),
        }

        Ok(())
    }

    #[test]
    fn test_rpc_response_err_encode() -> Result<()> {
        let response = RpcResponse::err(789, "something went wrong".to_string());
        let bytes = response.encode()?;

        // Verify basic structure
        let mut dec = Decoder::new(&bytes);
        let mut list = dec.list()?;

        assert_eq!(list.next()?.unwrap().as_u64()?, 789);

        match list.next()?.unwrap() {
            ValueDecoder::ResultErr => {
                assert_eq!(list.next()?.unwrap().as_str()?, "something went wrong");
            }
            _ => panic!("Expected ResultErr"),
        }

        Ok(())
    }
}
