// crates/isorpc/src/decode.rs
use isopack::Decoder;
use isopack::ValueDecoder;
use wasmtime::component::Type;
use wasmtime::component::Val;

use crate::types::Result;
use crate::types::Error;

/// Decodes a list of values from a ValueDecoder against a list of expected Types.
pub fn decode_vals(val_dec: ValueDecoder, types: &[Type]) -> Result<Vec<Val>> {
    let mut list = match val_dec {
        ValueDecoder::List(l) => l,
        _ => return Err(Error::TypeMismatch { expected: "List".into(), got: format!("{:?}", val_dec) }),
    };

    let mut values = Vec::with_capacity(types.len());

    for ty in types {
        let item_dec = list.next()?.ok_or_else(|| Error::Malformed("Not enough arguments".into()))?;
        values.push(convert_val(item_dec, ty)?);
    }

    // Ensure no extra arguments
    if list.next()?.is_some() {
        return Err(Error::Malformed("Too many arguments".into()));
    }

    Ok(values)
}

/// Converts a generic Isopack ValueDecoder into a Wasmtime Val based on Type.
pub fn convert_val(decoded: ValueDecoder, ty: &Type) -> Result<Val> {
    match (decoded, ty) {
        (ValueDecoder::Bool(b), Type::Bool) => Ok(Val::Bool(b)),
        (ValueDecoder::S8(i), Type::S8) => Ok(Val::S8(i)),
        (ValueDecoder::U8(u), Type::U8) => Ok(Val::U8(u)),
        (ValueDecoder::S16(i), Type::S16) => Ok(Val::S16(i)),
        (ValueDecoder::U16(u), Type::U16) => Ok(Val::U16(u)),
        (ValueDecoder::S32(i), Type::S32) => Ok(Val::S32(i)),
        (ValueDecoder::U32(u), Type::U32) => Ok(Val::U32(u)),
        (ValueDecoder::S64(i), Type::S64) => Ok(Val::S64(i)),
        (ValueDecoder::U64(u), Type::U64) => Ok(Val::U64(u)),
        (ValueDecoder::F32(f), Type::Float32) => Ok(Val::Float32(f)),
        (ValueDecoder::F64(f), Type::Float64) => Ok(Val::Float64(f)),
        (ValueDecoder::U32(c), Type::Char) => Ok(Val::Char(char::from_u32(c).ok_or(Error::Malformed("Invalid char".into()))?)),
        (ValueDecoder::Str(s), Type::String) => Ok(Val::String(s.to_string().into_boxed_str().to_string())),

        // Lists
        (ValueDecoder::List(mut list), Type::List(l)) => {
            let elem_ty = l.ty();
            let mut vals = Vec::new();
            while let Some(item) = list.next()? {
                vals.push(convert_val(item, &elem_ty)?);
            }
            Ok(Val::List(vals))
        }

        // Tuples
        (ValueDecoder::List(mut list), Type::Tuple(t)) => {
            let mut vals = Vec::new();
            for ty in t.types() {
                let item = list.next()?.ok_or_else(|| Error::Malformed("Tuple too short".into()))?;
                vals.push(convert_val(item, &ty)?);
            }
            Ok(Val::Tuple(vals))
        }

        // Records
        (ValueDecoder::List(mut list), Type::Record(r)) => {
            let mut vals = Vec::new();
            for field in r.fields() {
                let item = list.next()?.ok_or_else(|| Error::Malformed("Record field missing".into()))?;
                let val = convert_val(item, &field.ty)?;
                vals.push((field.name.to_string(), val));
            }
            Ok(Val::Record(vals))
        }

        // Options
        (ValueDecoder::Option(opt), Type::Option(o)) => {
            match opt {
                None => Ok(Val::Option(None)),
                Some(boxed_dec) => {
                    let val = convert_val(*boxed_dec, &o.ty())?;
                    Ok(Val::Option(Some(Box::new(val))))
                }
            }
        }

        // Results
        (ValueDecoder::Result(res), Type::Result(r)) => {
            match res {
                Ok(boxed_dec) => {
                    let val = if let Some(ok_ty) = r.ok() {
                        Some(Box::new(convert_val(*boxed_dec, &ok_ty)?))
                    } else {
                        if !boxed_dec.is_unit() { return Err(Error::TypeMismatch { expected: "Unit".into(), got: "Value".into() }); }
                        None
                    };
                    Ok(Val::Result(Ok(val)))
                }
                Err(boxed_dec) => {
                    let val = if let Some(err_ty) = r.err() {
                        Some(Box::new(convert_val(*boxed_dec, &err_ty)?))
                    } else {
                        if !boxed_dec.is_unit() { return Err(Error::TypeMismatch { expected: "Unit".into(), got: "Value".into() }); }
                        None
                    };
                    Ok(Val::Result(Err(val)))
                }
            }
        }

        // Variants
        (ValueDecoder::Variant(name, boxed_dec), Type::Variant(v)) => {
            let case = v.cases().find(|c| c.name == name)
                .ok_or_else(|| Error::Malformed(format!("Unknown variant case: {}", name)))?;

            let val = if let Some(ty) = case.ty {
                Some(Box::new(convert_val(*boxed_dec, &ty)?))
            } else {
                if !boxed_dec.is_unit() { return Err(Error::TypeMismatch { expected: "Unit".into(), got: "Value".into() }); }
                None
            };
            Ok(Val::Variant(name.to_string(), val))
        }

        // Enums (Isopack Variant with Unit payload)
        (ValueDecoder::Variant(name, boxed_dec), Type::Enum(e)) => {
            if !e.names().any(|n| n == name) {
                return Err(Error::Malformed(format!("Unknown enum case: {}", name)));
            }
            if !boxed_dec.is_unit() {
                return Err(Error::TypeMismatch { expected: "Unit".into(), got: "Value".into() });
            }
            Ok(Val::Enum(name.to_string()))
        }

        // Flags
        (ValueDecoder::List(mut list), Type::Flags(f)) => {
            let mut active = Vec::new();
            while let Some(item) = list.next()? {
                let s = item.as_str()?;
                if !f.names().any(|n| n == s) {
                    return Err(Error::Malformed(format!("Unknown flag: {}", s)));
                }
                active.push(s.to_string());
            }
            Ok(Val::Flags(active))
        }

        (dec, ty) => Err(Error::TypeMismatch {
            expected: format!("{:?}", ty),
            got: format!("{:?}", dec)
        }),
    }
}
