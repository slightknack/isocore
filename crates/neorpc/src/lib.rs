//! # NeoRPC
//!
//! A distinctively strict, schema-driven RPC protocol over Neopack.
//!
//! ## Architecture
//!
//! This library bridges the semantic richness of `wasmtime::component::Val` with the
//! structural rigor of `neopack`. It provides a ledger-like wire format where
//! every RPC interaction is verified against the state machine of the underlying encoder.

use neopack::{Encoder, Decoder, Error as NeoError};
use wasmtime::component::{Val, Type};

#[cfg(test)]
mod tests;

/// Operational failures within the RPC mechanism itself.
#[derive(Debug, Clone)]
pub enum RpcError {
    /// The underlying Neopack serialization failed.
    Serialization(NeoError),
    /// The wire types did not match the expected Wasmtime types.
    TypeMismatch { expected: String, found: String },
    /// A record was missing a required field.
    MissingField(String),
    /// An unknown enum variant or flag was encountered.
    UnknownVariant(String),
    /// The internal structure of the message was malformed (e.g., missing Sequence header).
    ProtocolViolation(String),
    /// Attempted to encode/decode a type not supported by RPC (Resource, Future, Stream).
    UnsupportedType(String),
}

impl From<NeoError> for RpcError {
    fn from(e: NeoError) -> Self { Self::Serialization(e) }
}

pub type Result<T> = std::result::Result<T, RpcError>;

// ============================================================================
//  CORE TYPES
// ============================================================================

/// Reasons for an RPC failure (The "Err" side of a Reply).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureReason {
    /// The component explicitly trapped (panic/abort).
    AppTrapped,
    /// Execution exhausted the fuel budget.
    OutOfFuel,
    /// Execution exceeded memory limits.
    OutOfMemory,
    /// The target instance ID was not found.
    InstanceNotFound,
    /// The method does not exist on the instance.
    MethodNotFound,
    /// Arguments provided did not match the method signature.
    BadArgumentCount,
    // Generic/Other failure with a description.
    // Other(String),
}

/// A partially decoded RPC Call header.
///
/// The arguments are held in a `Decoder` state, waiting for type information.
pub struct CallFrame<'a> {
    pub seq: u64,
    pub target: &'a str,
    pub method: &'a str,
    /// Use `decode_vals` with this decoder and the method signature.
    pub args_decoder: Decoder<'a>,
}

/// A partially decoded RPC Reply header.
pub struct ReplyFrame<'a> {
    pub seq: u64,
    /// The result of the call.
    /// - `Ok(Decoder)`: Success. Use `decode_vals` with the return types.
    /// - `Err(FailureReason)`: System failure.
    pub status: std::result::Result<Decoder<'a>, FailureReason>,
}

// ============================================================================
//  WIRE ENCODING
// ============================================================================

/// Encodes a full RPC Call message onto the wire.
pub fn encode_call(enc: &mut Encoder, seq: u64, target: &str, method: &str, args: &[Val]) -> Result<()> {
    enc.variant_begin("Call")?;
    enc.map_begin()?;

    // Header Fields
    write_map_u64(enc, "seq", seq)?;
    write_map_str(enc, "target", target)?;
    write_map_str(enc, "method", method)?;

    // Arguments List
    enc.variant_begin("args")?;
    enc.list_begin()?;
    for val in args {
        encode_val(enc, val)?;
    }
    enc.list_end()?; // List
    enc.variant_end()?; // Variant("args")

    enc.map_end()?; // Map
    enc.variant_end()?; // Variant("Call")
    Ok(())
}

/// Encodes a Successful Reply.
pub fn encode_reply_success(enc: &mut Encoder, seq: u64, results: &[Val]) -> Result<()> {
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

/// Encodes a Failure Reply.
pub fn encode_reply_failure(enc: &mut Encoder, seq: u64, reason: &FailureReason) -> Result<()> {
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
    enc.variant_end()?; // variant("reason")

    enc.map_end()?;
    enc.result_err_end()?;
    enc.variant_end()?;
    Ok(())
}

// Helpers for Map construction
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

fn val_desc(val: &Val) -> &'static str {
    match val {
        Val::Bool(_) => "bool",
        Val::U8(_) => "u8",
        Val::S8(_) => "s8",
        Val::U16(_) => "u16",
        Val::S16(_) => "s16",
        Val::U32(_) => "u32",
        Val::S32(_) => "s32",
        Val::U64(_) => "u64",
        Val::S64(_) => "s64",
        Val::Float32(_) => "f32",
        Val::Float64(_) => "f64",
        Val::Char(_) => "char",
        Val::String(_) => "string",
        Val::List(_) => "list",
        Val::Record(_) => "record",
        Val::Tuple(_) => "tuple",
        Val::Variant(..) => "variant",
        Val::Enum(_) => "enum",
        Val::Option(_) => "option",
        Val::Result(_) => "result",
        Val::Flags(_) => "flags",
        Val::Resource(_) => "resource",
        Val::Future(_) => "future",
        Val::Stream(_) => "stream",
        Val::ErrorContext(_) => "error-context",
    }
}

// ============================================================================
//  VALUE TRANSLATION (Val -> Neopack)
// ============================================================================

/// Encodes a `wasmtime::component::Val` into the encoder stream.
pub fn encode_val(enc: &mut Encoder, val: &Val) -> Result<()> {
    match val {
        Val::Bool(b) => enc.bool(*b)?,
        Val::U8(v) => enc.u8(*v)?,
        Val::U16(v) => enc.u16(*v)?,
        Val::U32(v) => enc.u32(*v)?,
        Val::U64(v) => enc.u64(*v)?,
        Val::S8(v) => enc.s8(*v)?,
        Val::S16(v) => enc.s16(*v)?,
        Val::S32(v) => enc.s32(*v)?,
        Val::S64(v) => enc.s64(*v)?,
        Val::Float32(v) => enc.f32(*v)?,
        Val::Float64(v) => enc.f64(*v)?,
        Val::Char(v) => enc.char(*v)?,
        Val::String(v) => enc.str(v)?,
        Val::List(items) => {
            enc.list_begin()?;
            for item in items {
                encode_val(enc, item)?;
            }
            enc.list_end()?;
        },
        Val::Record(fields) => {
            enc.map_begin()?;
            for (name, value) in fields {
                enc.variant_begin(name)?;
                encode_val(enc, value)?;
                enc.variant_end()?;
            }
            enc.map_end()?;
        },
        Val::Tuple(items) => {
            enc.list_begin()?;
            for item in items {
                encode_val(enc, item)?;
            }
            enc.list_end()?;
        },
        Val::Variant(name, value) => {
            enc.variant_begin(name)?;
            match value {
                Some(v) => encode_val(enc, v)?,
                None => enc.unit()?,
            }
            enc.variant_end()?;
        },
        Val::Enum(name) => {
            enc.variant_begin(name)?;
            enc.unit()?;
            enc.variant_end()?;
        },
        Val::Option(opt) => {
            match opt {
                Some(v) => {
                    enc.option_some_begin()?;
                    encode_val(enc, v)?;
                    enc.option_some_end()?;
                },
                None => enc.option_none()?,
            }
        },
        Val::Result(res) => {
            match res {
                Ok(Some(v)) => {
                    enc.result_ok_begin()?;
                    encode_val(enc, v)?;
                    enc.result_ok_end()?;
                },
                Ok(None) => {
                    enc.result_ok_begin()?;
                    enc.unit()?;
                    enc.result_ok_end()?;
                }
                Err(Some(v)) => {
                    enc.result_err_begin()?;
                    encode_val(enc, v)?;
                    enc.result_err_end()?;
                },
                Err(None) => {
                    enc.result_err_begin()?;
                    enc.unit()?;
                    enc.result_err_end()?;
                }
            }
        },
        Val::Flags(names) => {
            enc.list_begin()?;
            for name in names {
                enc.str(name)?;
            }
            enc.list_end()?;
        },
        Val::Resource(_) | Val::Future(_) | Val::Stream(_) | Val::ErrorContext(_) => {
            return Err(RpcError::UnsupportedType(val_desc(val).into()));
        }
    }
    Ok(())
}

// ============================================================================
//  WIRE DECODING
// ============================================================================

/// Peeks at the message to determine if it is a Call or Reply and decodes the frame.
pub enum RpcFrame<'a> {
    Call(CallFrame<'a>),
    Reply(ReplyFrame<'a>),
}

pub fn decode_frame<'a>(dec: &mut Decoder<'a>) -> Result<RpcFrame<'a>> {
    let (msg_type, mut body) = dec.variant()?;

    match msg_type {
        "Call" => {
            let mut map = body.map()?;
            let mut seq = None;
            let mut target = None;
            let mut method = None;
            let mut args_dec = None;

            while let Some((key, val)) = map.next()? {
                match key {
                    "seq" => seq = Some(val.clone().u64()?),
                    "target" => target = Some(val.clone().str()?),
                    "method" => method = Some(val.clone().str()?),
                    "args" => {
                        args_dec = Some(val);
                    }
                    _ => {}
                }
            }

            Ok(RpcFrame::Call(CallFrame {
                seq: seq.ok_or(RpcError::ProtocolViolation("Missing seq".into()))?,
                target: target.ok_or(RpcError::ProtocolViolation("Missing target".into()))?,
                method: method.ok_or(RpcError::ProtocolViolation("Missing method".into()))?,
                args_decoder: args_dec.ok_or(RpcError::ProtocolViolation("Missing args".into()))?,
            }))
        },
        "Reply" => {
            let res = body.result()?;
            match res {
                Ok(mut ok_body) => {
                    let mut map = ok_body.map()?;
                    let mut seq = None;
                    let mut results_dec = None;

                    while let Some((key, val)) = map.next()? {
                        match key {
                            "seq" => seq = Some(val.clone().u64()?),
                            "results" => results_dec = Some(val),
                            _ => {}
                        }
                    }
                    Ok(RpcFrame::Reply(ReplyFrame {
                        seq: seq.ok_or(RpcError::ProtocolViolation("Missing seq".into()))?,
                        status: Ok(results_dec.ok_or(RpcError::ProtocolViolation("Missing results".into()))?),
                    }))
                },
                Err(mut err_body) => {
                    let mut map = err_body.map()?;
                    let mut seq = None;
                    let mut reason = None;

                    while let Some((key, mut val)) = map.next()? {
                        match key {
                            "seq" => seq = Some(val.u64()?),
                            "reason" => {
                                let (r_type, mut r_val) = val.variant()?;
                                reason = Some(match r_type {
                                    "Trapped" => FailureReason::AppTrapped,
                                    "NoFuel" => FailureReason::OutOfFuel,
                                    "OOM" => FailureReason::OutOfMemory,
                                    "NoInstance" => FailureReason::InstanceNotFound,
                                    "NoMethod" => FailureReason::MethodNotFound,
                                    "BadArgs" => FailureReason::BadArgumentCount,
                                    _ => todo!() // this should exit early
                                });
                            }
                            _ => {}
                        }
                    }
                     Ok(RpcFrame::Reply(ReplyFrame {
                        seq: seq.ok_or(RpcError::ProtocolViolation("Missing seq".into()))?,
                        status: Err(reason), // todo
                    }))
                }
            }
        },
        _ => Err(RpcError::UnknownVariant(msg_type.to_string())),
    }
}

// ============================================================================
//  VALUE DECODING (Neopack -> Val)
// ============================================================================

/// Decodes a list of values given a list of expected types.
pub fn decode_vals(mut list_decoder: Decoder, types: &[Type]) -> Result<Vec<Val>> {
    let mut list_iter = list_decoder.list()?;
    let mut vals = Vec::with_capacity(types.len());

    for ty in types {
        if let Some(mut item_dec) = list_iter.next() {
            vals.push(decode_val(&mut item_dec, ty)?);
        } else {
            return Err(RpcError::ProtocolViolation("Fewer args than types".into()));
        }
    }

    if list_iter.next().is_some() {
        return Err(RpcError::ProtocolViolation("More args than types".into()));
    }

    Ok(vals)
}

/// Decodes a single Value based on the expected Wasmtime Type.
pub fn decode_val(dec: &mut Decoder, ty: &Type) -> Result<Val> {
    match ty {
        Type::Bool => Ok(Val::Bool(dec.bool()?)),
        Type::U8 => Ok(Val::U8(dec.u8()?)),
        Type::U16 => Ok(Val::U16(dec.u16()?)),
        Type::U32 => Ok(Val::U32(dec.u32()?)),
        Type::U64 => Ok(Val::U64(dec.u64()?)),
        Type::S8 => Ok(Val::S8(dec.s8()?)),
        Type::S16 => Ok(Val::S16(dec.s16()?)),
        Type::S32 => Ok(Val::S32(dec.s32()?)),
        Type::S64 => Ok(Val::S64(dec.s64()?)),
        Type::Float32 => Ok(Val::Float32(dec.f32()?)),
        Type::Float64 => Ok(Val::Float64(dec.f64()?)),
        Type::Char => Ok(Val::Char(dec.char()?)),
        Type::String => Ok(Val::String(dec.str()?.to_string())),

        Type::List(handle) => {
            let inner_ty = handle.ty();
            let mut iter = dec.list()?;
            let mut list = Vec::new();
            while let Some(mut item_dec) = iter.next() {
                list.push(decode_val(&mut item_dec, &inner_ty)?);
            }
            Ok(Val::List(list))
        },

        Type::Tuple(handle) => {
            let mut iter = dec.list()?;
            let mut list = Vec::new();
            for ty in handle.types() {
                let mut item = iter.next().ok_or(RpcError::ProtocolViolation("Tuple too short".into()))?;
                list.push(decode_val(&mut item, &ty)?);
            }
            Ok(Val::Tuple(list))
        },

        Type::Record(handle) => {
            let mut iter = dec.map()?;
            // Buffer map for random access
            let mut entries: Vec<(String, Decoder)> = Vec::new();
            while let Some((k, v)) = iter.next()? {
                entries.push((k.to_string(), v));
            }

            let mut record_vals = Vec::new();
            for field in handle.fields() {
                if let Some(idx) = entries.iter().position(|(k, _)| k == field.name) {
                    let (_, mut val_dec) = entries.remove(idx);
                    record_vals.push((field.name.to_string(), decode_val(&mut val_dec, &field.ty)?));
                } else {
                    return Err(RpcError::MissingField(field.name.to_string()));
                }
            }
            Ok(Val::Record(record_vals))
        },

        Type::Variant(handle) => {
            let (name, mut val_dec) = dec.variant()?;
            if let Some(case) = handle.cases().find(|c| c.name == name) {
                 let payload = if let Some(ty) = &case.ty {
                    Some(Box::new(decode_val(&mut val_dec, ty)?))
                } else {
                    val_dec.unit()?;
                    None
                };
                Ok(Val::Variant(name.to_string(), payload))
            } else {
                Err(RpcError::UnknownVariant(name.to_string()))
            }
        },

        Type::Enum(handle) => {
            let (name, mut val_dec) = dec.variant()?;
            val_dec.unit()?;
            if handle.names().any(|n| n == name) {
                Ok(Val::Enum(name.to_string()))
            } else {
                Err(RpcError::UnknownVariant(name.to_string()))
            }
        },

        Type::Option(handle) => {
            let inner_ty = handle.ty();
            if let Some(mut opt_dec) = dec.option()? {
                let val = decode_val(&mut opt_dec, &inner_ty)?;
                Ok(Val::Option(Some(Box::new(val))))
            } else {
                Ok(Val::Option(None))
            }
        },

        Type::Result(handle) => {
            match dec.result()? {
                Ok(mut d) => {
                    let val = if let Some(ty) = handle.ok() {
                        Some(Box::new(decode_val(&mut d, &ty)?))
                    } else {
                        d.unit()?; None
                    };
                    Ok(Val::Result(Ok(val)))
                },
                Err(mut d) => {
                    let val = if let Some(ty) = handle.err() {
                        Some(Box::new(decode_val(&mut d, &ty)?))
                    } else {
                        d.unit()?; None
                    };
                    Ok(Val::Result(Err(val)))
                }
            }
        },

        Type::Flags(handle) => {
            let mut iter = dec.list()?;
            let mut active = Vec::new();
            while let Some(mut item) = iter.next() {
                let f = item.str()?;
                if handle.names().any(|n| n == f) {
                    active.push(f.to_string());
                } else {
                    return Err(RpcError::UnknownVariant(f.to_string()));
                }
            }
            Ok(Val::Flags(active))
        },

        Type::Own(_) | Type::Borrow(_) | Type::Future(_) | Type::Stream(_) | Type::ErrorContext => {
            Err(RpcError::UnsupportedType("RPC does not support resources or handles".into()))
        },
    }
}
