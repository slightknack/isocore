//! # Protocol Frames
//!
//! Defines the structure of the RPC envelope (Call vs Reply).
//!
//! ## Invariants
//! - **Panic Safety**: All decoding paths return `Result`, never panicking on unknown data.
//! - **Forward Compatibility**: Unknown header fields are safely skipped.

use crate::codec::encode_val;
use crate::error::FailureReason;
use crate::error::Result;
use crate::error::RpcError;

use neopack::Decoder;
use neopack::Encoder;

use wasmtime::component::Val;

/// A partially decoded RPC Call header.
///
/// **Invariant**: The `args_decoder` points to a List container containing the arguments.
pub struct CallFrame<'a> {
    pub seq: u64,
    pub target: &'a str,
    pub method: &'a str,
    /// Use `decode_vals` with this decoder and the method signature.
    pub args_decoder: Decoder<'a>,
}

impl<'a> CallFrame<'a> {
    /// Encodes a Call frame onto the wire.
    pub fn encode(enc: &mut Encoder, seq: u64, target: &str, method: &str, args: &[Val]) -> Result<()> {
        enc.variant_begin("Call")?;
        enc.map_begin()?;

        write_map_u64(enc, "seq", seq)?;
        write_map_str(enc, "target", target)?;
        write_map_str(enc, "method", method)?;

        enc.variant_begin("args")?;
        enc.list_begin()?;
        for val in args {
            encode_val(enc, val)?;
        }
        enc.list_end()?;
        enc.variant_end()?;

        enc.map_end()?;
        enc.variant_end()?;
        Ok(())
    }

    /// Decodes a Call frame from the wire.
    pub fn decode(mut dec: Decoder<'a>) -> Result<Self> {
        let mut map = dec.map()?;
        let mut seq = None;
        let mut target = None;
        let mut method = None;
        let mut args_dec = None;

        while let Some((key, mut val)) = map.next()? {
            match key {
                "seq" => seq = Some(val.u64()?),
                "target" => target = Some(val.str()?),
                "method" => method = Some(val.str()?),
                "args" => args_dec = Some(val),
                _ => val.skip()?,
            }
        }

        Ok(CallFrame {
            seq: seq.ok_or(RpcError::ProtocolViolation("Missing seq".into()))?,
            target: target.ok_or(RpcError::ProtocolViolation("Missing target".into()))?,
            method: method.ok_or(RpcError::ProtocolViolation("Missing method".into()))?,
            args_decoder: args_dec.ok_or(RpcError::ProtocolViolation("Missing args".into()))?,
        })
    }
}

/// A partially decoded RPC Reply header.
pub struct ReplyFrame<'a> {
    pub seq: u64,
    /// The result of the call.
    /// - `Ok(Decoder)`: Success. Points to a List container of results.
    /// - `Err(FailureReason)`: System failure.
    pub status: std::result::Result<Decoder<'a>, FailureReason>,
}

impl<'a> ReplyFrame<'a> {
    /// Encodes a successful Reply frame onto the wire.
    pub fn encode_success(enc: &mut Encoder, seq: u64, results: &[Val]) -> Result<()> {
        enc.variant_begin("Reply")?;
        enc.result_ok_begin()?;
        enc.map_begin()?;

        write_map_u64(enc, "seq", seq)?;

        enc.variant_begin("results")?;
        enc.list_begin()?;
        for val in results {
            encode_val(enc, val)?;
        }
        enc.list_end()?;
        enc.variant_end()?;

        enc.map_end()?;
        enc.result_ok_end()?;
        enc.variant_end()?;
        Ok(())
    }

    /// Encodes a failure Reply frame onto the wire.
    pub fn encode_failure(enc: &mut Encoder, seq: u64, reason: &FailureReason) -> Result<()> {
        enc.variant_begin("Reply")?;
        enc.result_err_begin()?;
        enc.map_begin()?;

        write_map_u64(enc, "seq", seq)?;

        enc.variant_begin("reason")?;
        match reason {
            FailureReason::AppTrapped => { enc.variant_begin("Trapped")?; enc.unit()?; enc.variant_end()?; },
            FailureReason::OutOfFuel => { enc.variant_begin("NoFuel")?; enc.unit()?; enc.variant_end()?; },
            FailureReason::OutOfMemory => { enc.variant_begin("OOM")?; enc.unit()?; enc.variant_end()?; },
            FailureReason::InstanceNotFound => { enc.variant_begin("NoInstance")?; enc.unit()?; enc.variant_end()?; },
            FailureReason::MethodNotFound => { enc.variant_begin("NoMethod")?; enc.unit()?; enc.variant_end()?; },
            FailureReason::BadArgumentCount => { enc.variant_begin("BadArgs")?; enc.unit()?; enc.variant_end()?; },
        }
        enc.variant_end()?;

        enc.map_end()?;
        enc.result_err_end()?;
        enc.variant_end()?;
        Ok(())
    }

    /// Decodes a Reply frame from the wire.
    pub fn decode(mut dec: Decoder<'a>) -> Result<Self> {
        let res = dec.result()?;
        match res {
            Ok(ok_body) => decode_reply_success(ok_body),
            Err(err_body) => decode_reply_failure(err_body),
        }
    }
}

/// The top-level frame of an RPC message.
pub enum RpcFrame<'a> {
    Call(CallFrame<'a>),
    Reply(ReplyFrame<'a>),
}

impl<'a> RpcFrame<'a> {
    /// Decodes an RPC frame from the wire.
    pub fn decode(dec: &mut Decoder<'a>) -> Result<Self> {
        let (msg_type, body) = dec.variant()?;

        match msg_type {
            "Call" => Ok(RpcFrame::Call(CallFrame::decode(body)?)),
            "Reply" => Ok(RpcFrame::Reply(ReplyFrame::decode(body)?)),
            _ => Err(RpcError::UnknownVariant(format!("Top-level frame: {}", msg_type))),
        }
    }
}

fn decode_reply_success<'a>(mut ok_body: Decoder<'a>) -> Result<ReplyFrame<'a>> {
    let mut map = ok_body.map()?;
    let mut seq = None;
    let mut results_dec = None;

    while let Some((key, mut val)) = map.next()? {
        match key {
            "seq" => seq = Some(val.u64()?),
            "results" => results_dec = Some(val),
            _ => val.skip()?,
        }
    }

    Ok(ReplyFrame {
        seq: seq.ok_or(RpcError::ProtocolViolation("Missing seq".into()))?,
        status: Ok(results_dec.ok_or(RpcError::ProtocolViolation("Missing results".into()))?),
    })
}

fn decode_reply_failure<'a>(mut err_body: Decoder<'a>) -> Result<ReplyFrame<'a>> {
    let mut map = err_body.map()?;
    let mut seq = None;
    let mut reason = None;

    while let Some((key, mut val)) = map.next()? {
        match key {
            "seq" => seq = Some(val.u64()?),
            "reason" => {
                let (r_type, mut r_val) = val.variant()?;
                r_val.unit()?;
                
                reason = Some(match r_type {
                    "Trapped" => FailureReason::AppTrapped,
                    "NoFuel" => FailureReason::OutOfFuel,
                    "OOM" => FailureReason::OutOfMemory,
                    "NoInstance" => FailureReason::InstanceNotFound,
                    "NoMethod" => FailureReason::MethodNotFound,
                    "BadArgs" => FailureReason::BadArgumentCount,
                    other => return Err(RpcError::UnknownVariant(format!("FailureReason: {}", other))),
                });
            }
            _ => val.skip()?,
        }
    }

    Ok(ReplyFrame {
        seq: seq.ok_or(RpcError::ProtocolViolation("Missing seq".into()))?,
        status: Err(reason.ok_or(RpcError::ProtocolViolation("Missing reason".into()))?),
    })
}

fn write_map_u64(enc: &mut Encoder, key: &str, val: u64) -> Result<()> {
    enc.variant_begin(key)?;
    enc.u64(val)?;
    enc.variant_end()?;
    Ok(())
}

fn write_map_str(enc: &mut Encoder, key: &str, val: &str) -> Result<()> {
    enc.variant_begin(key)?;
    enc.str(val)?;
    enc.variant_end()?;
    Ok(())
}

/// Legacy function for backwards compatibility.
pub fn encode_call(enc: &mut Encoder, seq: u64, target: &str, method: &str, args: &[Val]) -> Result<()> {
    CallFrame::encode(enc, seq, target, method, args)
}

/// Legacy function for backwards compatibility.
pub fn encode_reply_success(enc: &mut Encoder, seq: u64, results: &[Val]) -> Result<()> {
    ReplyFrame::encode_success(enc, seq, results)
}

/// Legacy function for backwards compatibility.
pub fn encode_reply_failure(enc: &mut Encoder, seq: u64, reason: &FailureReason) -> Result<()> {
    ReplyFrame::encode_failure(enc, seq, reason)
}

/// Legacy function for backwards compatibility.
pub fn decode_frame<'a>(dec: &mut Decoder<'a>) -> Result<RpcFrame<'a>> {
    RpcFrame::decode(dec)
}
