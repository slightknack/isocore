//! # Codec
//!
//! The translation layer between `wasmtime::component::Val` and the `neopack` wire format.
//!
//! ## Invariants
//! - **Recursion Safety**: All recursive operations are bounded by `MAX_RECURSION_DEPTH`.
//! - **Type Strictness**: Decoding verifies wire tags against the expected `Type` signature.

use crate::error::Result;
use crate::error::Error;

use neopack::Decoder;
use neopack::Encoder;

use wasmtime::component::Type;
use wasmtime::component::Val;

/// The maximum nesting depth for Values before trapping.
const MAX_RECURSION_DEPTH: usize = 64;

/// Encodes a `wasmtime::component::Val` into the encoder stream.
///
/// # Errors
/// Returns `RpcError::RecursionLimitExceeded` if the value is too deeply nested.
pub fn encode_val(enc: &mut Encoder, val: &Val) -> Result<()> {
    encode_val_impl(enc, val, 0)
}

fn encode_val_impl(enc: &mut Encoder, val: &Val, depth: usize) -> Result<()> {
    if depth > MAX_RECURSION_DEPTH {
        return Err(Error::RecursionLimitExceeded);
    }

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
                encode_val_impl(enc, item, depth + 1)?;
            }
            enc.list_end()?;
        },
        Val::Record(fields) => {
            enc.map_begin()?;
            for (name, value) in fields {
                enc.variant_begin(name)?;
                encode_val_impl(enc, value, depth + 1)?;
                enc.variant_end()?;
            }
            enc.map_end()?;
        },
        Val::Tuple(items) => {
            enc.list_begin()?;
            for item in items {
                encode_val_impl(enc, item, depth + 1)?;
            }
            enc.list_end()?;
        },
        Val::Variant(name, value) => {
            enc.variant_begin(name)?;
            match value {
                Some(v) => encode_val_impl(enc, v, depth + 1)?,
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
                    encode_val_impl(enc, v, depth + 1)?;
                    enc.option_some_end()?;
                },
                None => enc.option_none()?,
            }
        },
        Val::Result(res) => {
            match res {
                Ok(Some(v)) => {
                    enc.result_ok_begin()?;
                    encode_val_impl(enc, v, depth + 1)?;
                    enc.result_ok_end()?;
                },
                Ok(None) => {
                    enc.result_ok_begin()?;
                    enc.unit()?;
                    enc.result_ok_end()?;
                }
                Err(Some(v)) => {
                    enc.result_err_begin()?;
                    encode_val_impl(enc, v, depth + 1)?;
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
            return Err(Error::UnsupportedType(val_desc(val).into()));
        }
    }
    Ok(())
}

/// Decodes a list of values given a list of expected types.
///
/// Generally used for decoding arguments lists or multiple return values.
pub fn decode_vals(mut list_decoder: Decoder, types: &[Type]) -> Result<Vec<Val>> {
    let mut list_iter = list_decoder.list()?;
    let mut vals = Vec::with_capacity(types.len());

    for ty in types {
        if let Some(mut item_dec) = list_iter.next() {
            vals.push(decode_val_impl(&mut item_dec, ty, 0)?);
        } else {
            return Err(Error::ProtocolViolation("Fewer args than types".into()));
        }
    }

    if list_iter.next().is_some() {
        return Err(Error::ProtocolViolation("More args than types".into()));
    }

    Ok(vals)
}

/// Decodes a single Value based on the expected Wasmtime Type.
pub fn decode_val(dec: &mut Decoder, ty: &Type) -> Result<Val> {
    decode_val_impl(dec, ty, 0)
}

fn decode_val_impl(dec: &mut Decoder, ty: &Type, depth: usize) -> Result<Val> {
    if depth > MAX_RECURSION_DEPTH {
        return Err(Error::RecursionLimitExceeded);
    }

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
                list.push(decode_val_impl(&mut item_dec, &inner_ty, depth + 1)?);
            }
            Ok(Val::List(list))
        },

        Type::Tuple(handle) => {
            let mut iter = dec.list()?;
            let mut list = Vec::new();
            for ty in handle.types() {
                let mut item = iter.next().ok_or(Error::ProtocolViolation("Tuple too short".into()))?;
                list.push(decode_val_impl(&mut item, &ty, depth + 1)?);
            }
            Ok(Val::Tuple(list))
        },

        Type::Record(handle) => {
            let fields: Vec<_> = handle.fields().collect();
            let field_count = fields.len();

            let mut record_vals = Vec::with_capacity(field_count);
            for _ in 0..field_count {
                record_vals.push(None);
            }

            let mut iter = dec.map()?;
            while let Some((k, mut v)) = iter.next()? {
                if let Some(idx) = fields.iter().position(|f| f.name == k) {
                    let field = &fields[idx];
                    let val = decode_val_impl(&mut v, &field.ty, depth + 1)?;
                    record_vals[idx] = Some((field.name.to_string(), val));
                } else {
                    v.skip()?;
                }
            }

            let mut result = Vec::with_capacity(field_count);
            for (idx, field) in fields.iter().enumerate() {
                if let Some(val) = record_vals[idx].take() {
                    result.push(val);
                } else {
                    return Err(Error::MissingField(field.name.to_string()));
                }
            }
            Ok(Val::Record(result))
        },

        Type::Variant(handle) => {
            let (name, mut val_dec) = dec.variant()?;
            if let Some(case) = handle.cases().find(|c| c.name == name) {
                 let payload = if let Some(ty) = &case.ty {
                    Some(Box::new(decode_val_impl(&mut val_dec, ty, depth + 1)?))
                } else {
                    val_dec.unit()?;
                    None
                };
                Ok(Val::Variant(name.to_string(), payload))
            } else {
                Err(Error::UnknownVariant(name.to_string()))
            }
        },

        Type::Enum(handle) => {
            let (name, mut val_dec) = dec.variant()?;
            val_dec.unit()?;
            if handle.names().any(|n| n == name) {
                Ok(Val::Enum(name.to_string()))
            } else {
                Err(Error::UnknownVariant(name.to_string()))
            }
        },

        Type::Option(handle) => {
            let inner_ty = handle.ty();
            if let Some(mut opt_dec) = dec.option()? {
                let val = decode_val_impl(&mut opt_dec, &inner_ty, depth + 1)?;
                Ok(Val::Option(Some(Box::new(val))))
            } else {
                Ok(Val::Option(None))
            }
        },

        Type::Result(handle) => {
            match dec.result()? {
                Ok(mut d) => {
                    let val = if let Some(ty) = handle.ok() {
                        Some(Box::new(decode_val_impl(&mut d, &ty, depth + 1)?))
                    } else {
                        d.unit()?; None
                    };
                    Ok(Val::Result(Ok(val)))
                },
                Err(mut d) => {
                    let val = if let Some(ty) = handle.err() {
                        Some(Box::new(decode_val_impl(&mut d, &ty, depth + 1)?))
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
                    return Err(Error::UnknownVariant(f.to_string()));
                }
            }
            Ok(Val::Flags(active))
        },

        Type::Own(_) | Type::Borrow(_) | Type::Future(_) | Type::Stream(_) | Type::ErrorContext => {
            Err(Error::UnsupportedType("RPC does not support resources or handles".into()))
        },
    }
}

/// Helper to get a string description of the Val type for errors.
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
