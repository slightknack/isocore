// crates/isorpc/src/message.rs
use isopack::Encoder;
use isopack::Decoder;
use isopack::ValueDecoder;
use isopack::traits::IsoWriter;
use wasmtime::component::Val;
use wasmtime::component::Type;

use crate::types::Result;
use crate::types::Error;
use crate::types::MessageHeader;
use crate::encode::encode_vals;
use crate::decode::decode_vals;

/// Encodes a Function Call message.
/// Format: Variant("call", [seq, method, [args...]])
pub fn encode_call(seq: u64, method: &str, args: &[Val]) -> Result<Vec<u8>> {
    let mut enc = Encoder::new();
    let mut variant = enc.variant("call")?;
    let mut list = variant.list()?;

    list.u64(seq)?;
    list.str(method)?;

    // Args are always nested in a list (tuple)
    encode_vals(args, &mut list)?;

    list.finish()?; // close list
    variant.finish()?; // close variant
    Ok(enc.into_bytes())
}

/// Encodes a Success Response message.
/// Format: Variant("resp", [seq, Result::Ok([vals...])])
pub fn encode_response_ok(seq: u64, values: &[Val]) -> Result<Vec<u8>> {
    let mut enc = Encoder::new();
    let mut variant = enc.variant("resp")?;
    let mut list = variant.list()?;

    list.u64(seq)?;

    let mut res = list.result_ok()?;
    // Wasm results are always a list/tuple of values
    encode_vals(values, &mut res)?;
    res.finish()?;

    list.finish()?;
    variant.finish()?;
    Ok(enc.into_bytes())
}

/// Encodes an Error Response message (Host Trap).
/// Format: Variant("resp", [seq, Result::Err(string)])
pub fn encode_response_err(seq: u64, error: &str) -> Result<Vec<u8>> {
    let mut enc = Encoder::new();
    let mut variant = enc.variant("resp")?;
    let mut list = variant.list()?;

    list.u64(seq)?;

    let mut res = list.result_err()?;
    res.str(error)?;
    res.finish()?;

    list.finish()?;
    variant.finish()?;
    Ok(enc.into_bytes())
}

/// Decodes the message header.
/// Returns a structure containing decoders positioned at the payload.
/// The caller must then call `decode_args` or `decode_result` with type info.
pub fn decode_header(bytes: &[u8]) -> Result<MessageHeader> {
    let mut dec = Decoder::new(bytes);
    let variant_name = dec.value()?.as_variant()?;
    let payload = dec.value()?;

    let mut list = match payload {
        ValueDecoder::List(l) => l,
        _ => return Err(Error::Malformed("Expected List payload".into())),
    };

    let seq = list.next()?.ok_or(Error::Malformed("Missing sequence".into()))?.as_u64()?;

    match variant_name {
        "call" => {
            let method = list.next()?.ok_or(Error::Malformed("Missing method".into()))?.as_str()?;
            let args_decoder = list.next()?.ok_or(Error::Malformed("Missing args".into()))?;

            Ok(MessageHeader::Call {
                seq,
                method,
                args_decoder
            })
        },
        "resp" => {
            let result_decoder = list.next()?.ok_or(Error::Malformed("Missing result".into()))?;

            Ok(MessageHeader::Response {
                seq,
                result_decoder
            })
        },
        _ => Err(Error::UnknownFunction(format!("Unknown message type: {}", variant_name)))
    }
}

impl<'a> MessageHeader<'a> {
    /// Finish decoding a Call by providing argument types.
    pub fn decode_args(self, types: &[Type]) -> Result<(u64, String, Vec<Val>)> {
        match self {
            MessageHeader::Call { seq, method, args_decoder } => {
                let vals = decode_vals(args_decoder, types)?;
                Ok((seq, method.to_string(), vals))
            },
            _ => Err(Error::Malformed("Called decode_args on Response".into())),
        }
    }

    /// Finish decoding a Response by providing expected return types.
    pub fn decode_result(self, types: &[Type]) -> Result<(u64, core::result::Result<Vec<Val>, String>)> {
        match self {
            MessageHeader::Response { seq, result_decoder } => {
                match result_decoder {
                    ValueDecoder::Result(res) => match res {
                        Ok(boxed_val) => {
                            // The inner value must be a list of returns
                            let vals = decode_vals(*boxed_val, types)?;
                            Ok((seq, Ok(vals)))
                        },
                        Err(boxed_val) => {
                            let err_msg = boxed_val.as_str()?.to_string();
                            Ok((seq, Err(err_msg)))
                        }
                    },
                    _ => Err(Error::Malformed("Response payload not a Result".into()))
                }
            },
            _ => Err(Error::Malformed("Called decode_result on Call".into())),
        }
    }
}
