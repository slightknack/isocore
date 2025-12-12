// crates/isorpc/src/encode.rs
use isopack::traits::IsoWriter;
use wasmtime::component::Val;

use crate::types::Result;
use crate::types::Error;

/// Encode a list of values (e.g. arguments or multi-value returns).
/// Encodes as an Isopack List.
pub fn encode_vals<W: IsoWriter>(vals: &[Val], writer: &mut W) -> Result<()> {
    let mut list = writer.list()?;
    for val in vals {
        encode_value(val, &mut list)?;
    }
    list.finish()?;
    Ok(())
}

/// Encode a single Wasm Value.
pub fn encode_value<W: IsoWriter>(val: &Val, writer: &mut W) -> Result<()> {
    match val {
        Val::Bool(v) => writer.bool(*v)?,
        Val::U8(v) => writer.u8(*v)?,
        Val::S8(v) => writer.i8(*v)?,
        Val::U16(v) => writer.u16(*v)?,
        Val::S16(v) => writer.i16(*v)?,
        Val::U32(v) => writer.u32(*v)?,
        Val::S32(v) => writer.i32(*v)?,
        Val::U64(v) => writer.u64(*v)?,
        Val::S64(v) => writer.i64(*v)?,
        Val::Float32(v) => writer.f32(*v)?,
        Val::Float64(v) => writer.f64(*v)?,
        Val::Char(v) => writer.u32(*v as u32)?,
        Val::String(v) => writer.str(v)?,

        Val::List(items) => {
            let mut list = writer.list()?;
            for item in items {
                encode_value(item, &mut list)?;
            }
            list.finish()?;
        }
        
        Val::Tuple(items) => {
            let mut list = writer.list()?;
            for item in items {
                encode_value(item, &mut list)?;
            }
            list.finish()?;
        }

        Val::Record(fields) => {
            let mut list = writer.list()?;
            for (_, value) in fields {
                encode_value(value, &mut list)?;
            }
            list.finish()?;
        }

        Val::Option(opt) => match opt {
            Some(v) => {
                let mut payload = writer.option_some()?;
                encode_value(v, &mut payload)?;
                payload.finish()?;
            }
            None => writer.option_none()?,
        },

        Val::Result(res) => match res {
            Ok(opt_val) => {
                let mut payload = writer.result_ok()?;
                match opt_val {
                    Some(v) => encode_value(v, &mut payload)?,
                    None => payload.unit()?,
                }
                payload.finish()?;
            }
            Err(opt_err) => {
                let mut payload = writer.result_err()?;
                match opt_err {
                    Some(e) => encode_value(e, &mut payload)?,
                    None => payload.unit()?,
                }
                payload.finish()?;
            }
        },

        Val::Variant(tag, opt_payload) => {
            let mut payload = writer.variant(tag)?;
            match opt_payload {
                Some(v) => encode_value(v, &mut payload)?,
                None => payload.unit()?,
            }
            payload.finish()?;
        }

        Val::Enum(tag) => {
            let mut payload = writer.variant(tag)?;
            payload.unit()?;
            payload.finish()?;
        }

        Val::Flags(names) => {
            let mut list = writer.list()?;
            for name in names {
                list.str(name)?;
            }
            list.finish()?;
        }

        Val::Resource(_) => return Err(Error::Malformed("Cannot encode Resource".into())),
        Val::Future(_) => return Err(Error::Malformed("Cannot encode Future".into())),
        Val::Stream(_) => return Err(Error::Malformed("Cannot encode Stream".into())),
        Val::ErrorContext(_) => return Err(Error::Malformed("Cannot encode ErrorContext".into())),
    }
    Ok(())
}
