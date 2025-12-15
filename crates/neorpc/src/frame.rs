//! # Protocol Frames
//!
//! Defines the structure of the RPC envelope (Call vs Reply).
//!
//! ## Invariants
//! - **Panic Safety**: All decoding paths return `Result`, never panicking on unknown data.
//! - **Forward Compatibility**: Unknown header fields are safely skipped.

use crate::error::FailureReason;
use crate::error::Result;
use crate::error::Error;

use neopack::Decoder;
use neopack::Encoder;

/// Encodes an outbound Call frame.
///
/// The `args_payload` is expected to be a pre-encoded neopack list of values,
/// as produced by `crate::codec::encode_vals_to_bytes()`.
pub struct CallEncoder<'a> {
    pub seq: u64,
    pub target: &'a str,
    pub method: &'a str,
    /// Pre-encoded arguments list (including list headers).
    pub args_payload: &'a [u8],
}

impl<'a> CallEncoder<'a> {
    pub fn new(seq: u64, target: &'a str, method: &'a str, args_payload: &'a [u8]) -> Self {
        Self { seq, target, method, args_payload }
    }

    /// Encode this call into the encoder.
    pub fn encode(&self, enc: &mut Encoder) -> Result<()> {
        enc.variant_begin("Call")?;
        enc.map_begin()?;

        write_map_u64(enc, "seq", self.seq)?;
        write_map_str(enc, "target", self.target)?;
        write_map_str(enc, "method", self.method)?;

        enc.variant_begin("args")?;
        enc.append_raw(self.args_payload)?;
        enc.variant_end()?;

        enc.map_end()?;
        enc.variant_end()?;
        Ok(())
    }
}

/// Decodes an inbound Call frame.
///
/// **Invariant**: The `args` decoder points to a List container containing the arguments.
pub struct CallDecoder<'a> {
    pub seq: u64,
    pub target: &'a str,
    pub method: &'a str,
    /// Use `decode_vals` with this decoder and the method signature.
    pub args: Decoder<'a>,
}

impl<'a> CallDecoder<'a> {
    /// Decode a Call frame from the decoder.
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

        Ok(CallDecoder {
            seq: seq.ok_or(Error::ProtocolViolation("Missing seq".into()))?,
            target: target.ok_or(Error::ProtocolViolation("Missing target".into()))?,
            method: method.ok_or(Error::ProtocolViolation("Missing method".into()))?,
            args: args_dec.ok_or(Error::ProtocolViolation("Missing args".into()))?,
        })
    }
}

/// Encodes an outbound Reply frame (success).
///
/// The `results_payload` is expected to be a pre-encoded neopack list of values,
/// as produced by `crate::codec::encode_vals_to_bytes()`.
pub struct ReplyOkEncoder<'a> {
    pub seq: u64,
    /// Pre-encoded results list (including list headers).
    pub results_payload: &'a [u8],
}

impl<'a> ReplyOkEncoder<'a> {
    pub fn new(seq: u64, results_payload: &'a [u8]) -> Self {
        Self { seq, results_payload }
    }

    /// Encode this success reply into the encoder.
    pub fn encode(&self, enc: &mut Encoder) -> Result<()> {
        enc.variant_begin("Reply")?;
        enc.result_ok_begin()?;
        enc.map_begin()?;

        write_map_u64(enc, "seq", self.seq)?;

        enc.variant_begin("results")?;
        enc.append_raw(self.results_payload)?;
        enc.variant_end()?;

        enc.map_end()?;
        enc.result_ok_end()?;
        enc.variant_end()?;
        Ok(())
    }
}

/// Encodes an outbound Reply frame (failure).
pub struct ReplyErrEncoder {
    pub seq: u64,
    pub reason: FailureReason,
}

impl ReplyErrEncoder {
    pub fn new(seq: u64, reason: FailureReason) -> Self {
        Self { seq, reason }
    }

    /// Encode this failure reply into the encoder.
    pub fn encode(&self, enc: &mut Encoder) -> Result<()> {
        enc.variant_begin("Reply")?;
        enc.result_err_begin()?;
        enc.map_begin()?;

        write_map_u64(enc, "seq", self.seq)?;

        enc.variant_begin("reason")?;
        encode_failure_reason(enc, &self.reason)?;
        enc.variant_end()?;

        enc.map_end()?;
        enc.result_err_end()?;
        enc.variant_end()?;
        Ok(())
    }
}

/// Decodes an inbound Reply frame.
pub struct ReplyDecoder<'a> {
    pub seq: u64,
    /// The result of the call.
    /// - `Ok(Decoder)`: Success. Points to a List container of results.
    /// - `Err(FailureReason)`: System failure.
    pub status: std::result::Result<Decoder<'a>, FailureReason>,
}

impl<'a> ReplyDecoder<'a> {
    /// Decode a Reply frame from the decoder.
    pub fn decode(mut dec: Decoder<'a>) -> Result<Self> {
        let res = dec.result()?;
        match res {
            Ok(ok_body) => Self::decode_success(ok_body),
            Err(err_body) => Self::decode_failure(err_body),
        }
    }

    fn decode_success(mut ok_body: Decoder<'a>) -> Result<Self> {
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

        Ok(ReplyDecoder {
            seq: seq.ok_or(Error::ProtocolViolation("Missing seq".into()))?,
            status: Ok(results_dec.ok_or(Error::ProtocolViolation("Missing results".into()))?),
        })
    }

    fn decode_failure(mut err_body: Decoder<'a>) -> Result<Self> {
        let mut map = err_body.map()?;
        let mut seq = None;
        let mut reason = None;

        while let Some((key, mut val)) = map.next()? {
            match key {
                "seq" => seq = Some(val.u64()?),
                "reason" => {
                    reason = Some(decode_failure_reason(&mut val)?);
                }
                _ => val.skip()?,
            }
        }

        Ok(ReplyDecoder {
            seq: seq.ok_or(Error::ProtocolViolation("Missing seq".into()))?,
            status: Err(reason.ok_or(Error::ProtocolViolation("Missing reason".into()))?),
        })
    }
}

/// Top-level frame decoder.
pub enum RpcFrame<'a> {
    Call(CallDecoder<'a>),
    Reply(ReplyDecoder<'a>),
}

impl<'a> RpcFrame<'a> {
    /// Decode an RPC frame from the decoder.
    pub fn decode(dec: &mut Decoder<'a>) -> Result<Self> {
        let (msg_type, body) = dec.variant()?;
        match msg_type {
            "Call" => Ok(RpcFrame::Call(CallDecoder::decode(body)?)),
            "Reply" => Ok(RpcFrame::Reply(ReplyDecoder::decode(body)?)),
            _ => Err(Error::UnknownVariant(format!("Top-level frame: {}", msg_type))),
        }
    }
}

/// Decodes just the sequence number from a raw frame.
/// This is useful for routing replies when the full decoding might fail.
pub fn decode_seq(bytes: &[u8]) -> Result<u64> {
    let mut dec = Decoder::new(bytes);
    let (msg_type, mut body) = dec.variant()?;
    let mut map = match msg_type {
        "Call" => body.map()?,
        "Reply" => match body.result()? {
            Ok(mut ok_body) => ok_body.map()?,
            Err(mut err_body) => err_body.map()?,
        },
        _ => return Err(Error::UnknownVariant(format!("Top-level frame: {}", msg_type))),
    };

    while let Some((key, mut val)) = map.next()? {
        if key == "seq" {
            return Ok(val.u64()?);
        } else {
            val.skip()?;
        }
    }

    Err(Error::ProtocolViolation("Missing seq".into()))
}

// Helper functions

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

/// Encode a unit variant (variant with no payload).
fn encode_unit_variant(enc: &mut Encoder, tag: &str) -> Result<()> {
    enc.variant_begin(tag)?;
    enc.unit()?;
    enc.variant_end()?;
    Ok(())
}

/// Decode a unit variant and return its tag.
fn decode_unit_variant<'a>(dec: &mut Decoder<'a>) -> Result<&'a str> {
    let (tag, mut body) = dec.variant()?;
    body.unit()?;
    Ok(tag)
}

/// Encode a FailureReason, including any payload for DomainSpecific.
fn encode_failure_reason(enc: &mut Encoder, reason: &FailureReason) -> Result<()> {
    match reason {
        FailureReason::DomainSpecific(code, msg) => {
            enc.variant_begin("Domain")?;
            enc.list_begin()?;
            enc.u32(*code)?;
            enc.str(msg)?;
            enc.list_end()?;
            enc.variant_end()?;
        }
        _ => {
            encode_unit_variant(enc, reason.as_tag())?;
        }
    }
    Ok(())
}

/// Decode a FailureReason, including any payload for DomainSpecific.
fn decode_failure_reason(dec: &mut Decoder) -> Result<FailureReason> {
    let (tag, mut body) = dec.variant()?;
    match tag {
        "Domain" => {
            let mut list_iter = body.list()?;
            let mut code_dec = list_iter.next().ok_or(Error::ProtocolViolation("Missing code in Domain".into()))?;
            let code = code_dec.u32()?;
            let mut msg_dec = list_iter.next().ok_or(Error::ProtocolViolation("Missing msg in Domain".into()))?;
            let msg = msg_dec.str()?.to_string();
            Ok(FailureReason::DomainSpecific(code, msg))
        }
        _ => {
            body.unit()?;
            FailureReason::from_tag(tag)
        }
    }
}
