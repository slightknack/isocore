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
use crate::error::Error;

use neopack::Decoder;
use neopack::Encoder;

use wasmtime::component::Val;

/// Encodes an outbound Call frame.
pub struct CallEncoder<'a> {
    pub seq: u64,
    pub target: &'a str,
    pub method: &'a str,
    pub args: &'a [Val],
}

impl<'a> CallEncoder<'a> {
    pub fn new(seq: u64, target: &'a str, method: &'a str, args: &'a [Val]) -> Self {
        Self { seq, target, method, args }
    }

    /// Encode this call into the encoder.
    pub fn encode(&self, enc: &mut Encoder) -> Result<()> {
        enc.variant_begin("Call")?;
        enc.map_begin()?;

        write_map_u64(enc, "seq", self.seq)?;
        write_map_str(enc, "target", self.target)?;
        write_map_str(enc, "method", self.method)?;

        enc.variant_begin("args")?;
        enc.list_begin()?;
        for val in self.args {
            encode_val(enc, val)?;
        }
        enc.list_end()?;
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
pub struct ReplyOkEncoder<'a> {
    pub seq: u64,
    pub results: &'a [Val],
}

impl<'a> ReplyOkEncoder<'a> {
    pub fn new(seq: u64, results: &'a [Val]) -> Self {
        Self { seq, results }
    }

    /// Encode this success reply into the encoder.
    pub fn encode(&self, enc: &mut Encoder) -> Result<()> {
        enc.variant_begin("Reply")?;
        enc.result_ok_begin()?;
        enc.map_begin()?;

        write_map_u64(enc, "seq", self.seq)?;

        enc.variant_begin("results")?;
        enc.list_begin()?;
        for val in self.results {
            encode_val(enc, val)?;
        }
        enc.list_end()?;
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
        encode_unit_variant(enc, self.reason.as_tag())?;
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
                    let tag = decode_unit_variant(&mut val)?;
                    reason = Some(FailureReason::from_tag(tag)?);
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
