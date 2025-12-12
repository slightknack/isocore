//! RPC encoding/decoding using the "Everything is a List" protocol.
//!
//! RPC calls and responses are encoded as lists:
//! - RpcCall: [sequence_number, function_name, [arg1, arg2, ...]]
//! - RpcResponse: [sequence_number, result]
//!   where result is Result<Val, Error>

use crate::encode::encode_val;
use isopack::traits::IsoWriter;
use isopack::types::Result;
use isopack::Encoder;
use wasmtime::component::Val;

/// Encode an RPC call as: [seq, func_name, [args...]]
pub fn encode_call(seq: u64, func_name: &str, args: &[Val]) -> Result<Vec<u8>> {
    let mut enc = Encoder::new();
    let mut list = enc.list()?;
    
    // Sequence number
    list.u64(seq)?;
    
    // Function name (max 32 bytes enforced by isopack)
    list.str(func_name)?;
    
    // Arguments as a nested list
    let mut args_list = list.list()?;
    for arg in args {
        encode_val(arg, &mut args_list)?;
    }
    args_list.finish()?;
    
    list.finish()?;
    Ok(enc.into_bytes())
}

/// Encode an RPC response as: [seq, Result<val, error_msg>]
pub fn encode_response_ok(seq: u64, value: &Val) -> Result<Vec<u8>> {
    let mut enc = Encoder::new();
    let mut list = enc.list()?;
    
    // Sequence number
    list.u64(seq)?;
    
    // Result::Ok(value)
    let mut result = list.result_ok()?;
    encode_val(value, &mut result)?;
    result.finish()?;
    
    list.finish()?;
    Ok(enc.into_bytes())
}

/// Encode an RPC error response as: [seq, Result::Err(error_msg)]
pub fn encode_response_err(seq: u64, error: &str) -> Result<Vec<u8>> {
    let mut enc = Encoder::new();
    let mut list = enc.list()?;
    
    // Sequence number
    list.u64(seq)?;
    
    // Result::Err(error_msg)
    let mut result = list.result_err()?;
    result.str(error)?;
    result.finish()?;
    
    list.finish()?;
    Ok(enc.into_bytes())
}
