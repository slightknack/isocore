//! RPC serialization and deserialization using isopack

use anyhow::Result;
use isopack::Decoder;
use isopack::Encoder;

// Extension trait to convert isopack::Result to anyhow::Result
trait IsopackResultExt<T> {
    fn ctx(self) -> Result<T>;
}

impl<T> IsopackResultExt<T> for isopack::Result<T> {
    fn ctx(self) -> Result<T> {
        self.map_err(|e| anyhow::anyhow!("Isopack error: {:?}", e))
    }
}

/// RPC call payload structure
#[derive(Debug, Clone, PartialEq)]
pub struct RpcCall {
    /// The remote instance identifier
    pub remote_instance: String,
    /// The interface name (e.g., "test:demo/math")
    pub interface: String,
    /// The function name (e.g., "add")
    pub function: String,
    /// The serialized arguments
    pub args: Vec<u8>,
}

impl RpcCall {
    /// Create a new RPC call
    pub fn new(remote_instance: String, interface: String, function: String, args: Vec<u8>) -> Self {
        Self {
            remote_instance,
            interface,
            function,
            args,
        }
    }

    /// Serialize the RPC call to bytes using isopack
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut encoder = Encoder::new();
        encoder.str(&self.remote_instance).ctx()?;
        encoder.str(&self.interface).ctx()?;
        encoder.str(&self.function).ctx()?;
        encoder.bytes(&self.args).ctx()?;
        Ok(encoder.into_bytes())
    }

    /// Deserialize an RPC call from bytes using isopack
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(bytes);
        let remote_instance = decoder.str().ctx()?.to_string();
        let interface = decoder.str().ctx()?.to_string();
        let function = decoder.str().ctx()?.to_string();
        let args = decoder.bytes().ctx()?.to_vec();
        Ok(Self { remote_instance, interface, function, args })
    }
}

/// RPC response payload structure
#[derive(Debug, Clone, PartialEq)]
pub struct RpcResponse {
    /// The serialized result
    pub result: Vec<u8>,
}

impl RpcResponse {
    /// Create a new RPC response
    pub fn new(result: Vec<u8>) -> Self {
        Self { result }
    }

    /// Serialize the RPC response to bytes using isopack
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut encoder = Encoder::new();
        encoder.bytes(&self.result).ctx()?;
        Ok(encoder.into_bytes())
    }

    /// Deserialize an RPC response from bytes using isopack
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(bytes);
        let result = decoder.bytes().ctx()?.to_vec();
        Ok(Self { result })
    }
}

/// Encode function arguments for math::add
pub fn encode_math_add_args(a: u32, b: u32) -> Result<Vec<u8>> {
    let mut encoder = Encoder::new();
    let mut record_data = Vec::new();
    record_data.extend_from_slice(&a.to_le_bytes());
    record_data.extend_from_slice(&b.to_le_bytes());
    encoder.record_raw(&record_data).ctx()?;
    Ok(encoder.into_bytes())
}

/// Decode function arguments for math::add
pub fn decode_math_add_args(bytes: &[u8]) -> Result<(u32, u32)> {
    let mut decoder = Decoder::new(bytes);
    let mut rec = decoder.record().ctx()?;
    let a = rec.u32().ctx()?;
    let b = rec.u32().ctx()?;
    Ok((a, b))
}

/// Encode function result for math::add
pub fn encode_math_add_result(result: u32) -> Result<Vec<u8>> {
    let mut encoder = Encoder::new();
    encoder.u32(result).ctx()?;
    Ok(encoder.into_bytes())
}

/// Decode function result for math::add
pub fn decode_math_add_result(bytes: &[u8]) -> Result<u32> {
    let mut decoder = Decoder::new(bytes);
    let result = decoder.u32().ctx()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rpc_call_roundtrip() {
        let call = RpcCall::new(
            "instance-123".to_string(),
            "test:demo/math".to_string(),
            "add".to_string(),
            vec![1, 2, 3, 4],
        );

        let bytes = call.to_bytes().unwrap();
        let decoded = RpcCall::from_bytes(&bytes).unwrap();

        assert_eq!(call, decoded);
    }

    #[test]
    fn test_rpc_response_roundtrip() {
        let response = RpcResponse::new(vec![42, 0, 0, 0]);

        let bytes = response.to_bytes().unwrap();
        let decoded = RpcResponse::from_bytes(&bytes).unwrap();

        assert_eq!(response, decoded);
    }

    #[test]
    fn test_math_add_args_roundtrip() {
        let a = 10u32;
        let b = 5u32;

        let bytes = encode_math_add_args(a, b).unwrap();
        let (decoded_a, decoded_b) = decode_math_add_args(&bytes).unwrap();

        assert_eq!(a, decoded_a);
        assert_eq!(b, decoded_b);
    }

    #[test]
    fn test_math_add_result_roundtrip() {
        let result = 15u32;

        let bytes = encode_math_add_result(result).unwrap();
        let decoded = decode_math_add_result(&bytes).unwrap();

        assert_eq!(result, decoded);
    }

    #[test]
    fn test_full_rpc_roundtrip() {
        // Encode arguments
        let args = encode_math_add_args(10, 5).unwrap();

        // Create RPC call
        let call = RpcCall::new(
            "math-service".to_string(),
            "test:demo/math".to_string(),
            "add".to_string(),
            args,
        );

        // Serialize call
        let call_bytes = call.to_bytes().unwrap();

        // Deserialize call
        let decoded_call = RpcCall::from_bytes(&call_bytes).unwrap();

        // Decode arguments
        let (a, b) = decode_math_add_args(&decoded_call.args).unwrap();
        assert_eq!(a, 10);
        assert_eq!(b, 5);

        // Simulate execution and encode result
        let result = a + b;
        let result_bytes = encode_math_add_result(result).unwrap();

        // Create RPC response
        let response = RpcResponse::new(result_bytes);

        // Serialize response
        let response_bytes = response.to_bytes().unwrap();

        // Deserialize response
        let decoded_response = RpcResponse::from_bytes(&response_bytes).unwrap();

        // Decode result
        let final_result = decode_math_add_result(&decoded_response.result).unwrap();
        assert_eq!(final_result, 15);
    }
}
