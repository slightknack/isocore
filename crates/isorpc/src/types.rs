//! RPC type definitions.

use isopack::Decoder;
use isopack::ValueDecoder;
use isopack::types::Result;
use wasmtime::component::Val;

/// RPC call structure.
///
/// Encoded as: [seq, func_name, [args...]]
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
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        crate::encode_call(self.seq, &self.function, &self.args)
    }

    /// Decode an RPC call from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let mut dec = Decoder::new(bytes);
        let mut list = dec.list()?;

        // Sequence number
        let seq = list.next()?.ok_or(isopack::types::Error::Malformed)?.as_u64()?;

        // Function name
        let function = list.next()?.ok_or(isopack::types::Error::Malformed)?.as_str()?.to_string();

        // Arguments list
        let mut args_list = match list.next()?.ok_or(isopack::types::Error::Malformed)? {
            ValueDecoder::List(l) => l,
            _ => return Err(isopack::types::Error::TypeMismatch),
        };

        let mut args = Vec::new();
        // Note: Full deserialization requires Type information
        // For now, this is a placeholder - actual usage should use decode_with_types
        while let Some(_) = args_list.next()? {
            // Skip for now - need Type info to deserialize properly
        }

        Ok(Self { seq, function, args })
    }
}

/// RPC response structure.
///
/// Encoded as: [seq, Result<val, error>]
#[derive(Debug, Clone)]
pub struct RpcResponse {
    /// Sequence number matching the request
    pub seq: u64,
    /// Response result
    pub result: core::result::Result<Val, String>,
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
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        match &self.result {
            Ok(val) => crate::encode_response_ok(self.seq, val),
            Err(err) => crate::encode_response_err(self.seq, err),
        }
    }

    /// Decode an RPC response from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let mut dec = Decoder::new(bytes);
        let mut list = dec.list()?;

        // Sequence number
        let seq = list.next()?.ok_or(isopack::types::Error::Malformed)?.as_u64()?;

        // Result<Val, String>
        let result = match list.next()?.ok_or(isopack::types::Error::Malformed)? {
            ValueDecoder::ResultOk => {
                // For now, return a placeholder
                // Full deserialization requires Type information
                Ok(Val::Unit)
            }
            ValueDecoder::ResultErr => {
                let error = list.next()?.ok_or(isopack::types::Error::Malformed)?.as_str()?.to_string();
                Err(error)
            }
            _ => return Err(isopack::types::Error::TypeMismatch),
        };

        Ok(Self { seq, result })
    }
}
