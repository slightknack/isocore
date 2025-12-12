//! Decoding of wasmtime component model values.

use isopack::Decoder;
use isopack::ValueDecoder;
use isopack::types::Result;
use wasmtime::component::Type;
use wasmtime::component::Val;

/// Decode a Val from bytes using the provided type information.
///
/// # Errors
///
/// Returns an error if:
/// - The bytes don't match the expected type
/// - The data is malformed
pub fn decode_val(bytes: &[u8], ty: &Type) -> Result<Val> {
    let mut dec = Decoder::new(bytes);
    decode_val_from_decoder(&mut dec, ty)
}

fn decode_val_from_decoder(dec: &mut Decoder, ty: &Type) -> Result<Val> {
    match ty {
        Type::Bool => Ok(Val::Bool(dec.bool()?)),
        Type::S8 => Ok(Val::S8(dec.i8()?)),
        Type::U8 => Ok(Val::U8(dec.u8()?)),
        Type::S16 => Ok(Val::S16(dec.i16()?)),
        Type::U16 => Ok(Val::U16(dec.u16()?)),
        Type::S32 => Ok(Val::S32(dec.i32()?)),
        Type::U32 => Ok(Val::U32(dec.u32()?)),
        Type::S64 => Ok(Val::S64(dec.i64()?)),
        Type::U64 => Ok(Val::U64(dec.u64()?)),
        Type::Float32 => Ok(Val::Float32(dec.f32()?)),
        Type::Float64 => Ok(Val::Float64(dec.f64()?)),
        Type::Char => Ok(Val::Char(char::from_u32(dec.u32()?).ok_or(isopack::types::Error::Malformed)?)),
        Type::String => Ok(Val::String(dec.str()?.into())),

        Type::List(elem_ty) => {
            let mut list_dec = dec.list()?;
            let mut items = Vec::new();
            while let Some(value_dec) = list_dec.next()? {
                // Need to deserialize based on element type
                // This is tricky because we've already consumed the value
                // TODO: Need to refactor to peek at value first
                return Err(isopack::types::Error::Malformed);
            }
            Ok(Val::List(items))
        }

        Type::Record(_) => {
            // TODO: Implement record deserialization
            Err(isopack::types::Error::Malformed)
        }

        Type::Tuple(_) => {
            // TODO: Implement tuple deserialization  
            Err(isopack::types::Error::Malformed)
        }

        Type::Variant(_) => {
            // TODO: Implement variant deserialization
            Err(isopack::types::Error::Malformed)
        }

        Type::Enum(_) => {
            // TODO: Implement enum deserialization
            Err(isopack::types::Error::Malformed)
        }

        Type::Option(inner_ty) => {
            match dec.value()? {
                ValueDecoder::OptionNone => Ok(Val::Option(None)),
                ValueDecoder::OptionSome => {
                    let inner = decode_val_from_decoder(dec, inner_ty)?;
                    Ok(Val::Option(Some(Box::new(inner))))
                }
                _ => Err(isopack::types::Error::TypeMismatch),
            }
        }

        Type::Result(result_ty) => {
            match dec.value()? {
                ValueDecoder::ResultOk => {
                    let val = if let Some(ok_ty) = result_ty.ok() {
                        Some(Box::new(decode_val_from_decoder(dec, ok_ty)?))
                    } else {
                        match dec.value()? {
                            ValueDecoder::Unit => None,
                            _ => return Err(isopack::types::Error::TypeMismatch),
                        }
                    };
                    Ok(Val::Result(Ok(val)))
                }
                ValueDecoder::ResultErr => {
                    let val = if let Some(err_ty) = result_ty.err() {
                        Some(Box::new(decode_val_from_decoder(dec, err_ty)?))
                    } else {
                        match dec.value()? {
                            ValueDecoder::Unit => None,
                            _ => return Err(isopack::types::Error::TypeMismatch),
                        }
                    };
                    Ok(Val::Result(Err(val)))
                }
                _ => Err(isopack::types::Error::TypeMismatch),
            }
        }

        Type::Flags(_) => {
            // Decode flags from list of strings
            let mut list_dec = dec.list()?;
            let mut names = Vec::new();
            while let Some(value_dec) = list_dec.next()? {
                let s = value_dec.as_str()?;
                names.push(s.into());
            }
            Ok(Val::Flags(names))
        }

        Type::Own(_) | Type::Borrow(_) => {
            // Resources cannot be deserialized
            Err(isopack::types::Error::Malformed)
        }
    }
}
