//! Serialization of wasmtime component model values.

use isopack::traits::IsoWriter;
use isopack::types::Error;
use isopack::types::Result;
use wasmtime::component::Val;

/// Encode a wasmtime Val using any IsoWriter implementation.
///
/// This function recursively encodes Wasm component model values into the isopack format.
/// It handles all Val types including primitives, containers (List, Tuple, Record), and
/// ADTs (Option, Result, Variant, Enum, Flags).
///
/// The generic implementation works with any writer type (Encoder, ListEncoder, ValueEncoder)
/// thanks to the IsoWriter trait with AdtTarget GAT support.
///
/// # Errors
///
/// Returns an error if:
/// - The writer encounters a serialization error
/// - A Resource type is encountered (Resources cannot be serialized)
/// - A variant or enum name exceeds 32 bytes
pub fn encode_val<W: IsoWriter>(val: &Val, writer: &mut W) -> Result<()> {
    match val {
        // Primitives
        Val::Bool(v) => writer.bool(*v),
        Val::U8(v) => writer.u8(*v),
        Val::S8(v) => writer.i8(*v),
        Val::U16(v) => writer.u16(*v),
        Val::S16(v) => writer.i16(*v),
        Val::U32(v) => writer.u32(*v),
        Val::S32(v) => writer.i32(*v),
        Val::U64(v) => writer.u64(*v),
        Val::S64(v) => writer.i64(*v),
        Val::Float32(v) => writer.f32(*v),
        Val::Float64(v) => writer.f64(*v),
        Val::Char(v) => writer.u32(*v as u32),
        Val::String(v) => writer.str(v),

        // Containers - encode as Lists
        Val::List(items) | Val::Tuple(items) => {
            let mut list = writer.list()?;
            for item in items {
                encode_val(item, &mut list)?;
            }
            list.finish()
        }

        Val::Record(fields) => {
            let mut list = writer.list()?;
            for (_, value) in fields {
                encode_val(value, &mut list)?;
            }
            list.finish()
        }

        // ADTs - Option
        Val::Option(opt) => match opt {
            Some(v) => {
                let mut payload = writer.option_some()?;
                encode_val(v, &mut payload)?;
                payload.finish()
            }
            None => writer.option_none(),
        },

        // ADTs - Result
        Val::Result(res) => match res {
            Ok(opt_val) => {
                let mut payload = writer.result_ok()?;
                match opt_val {
                    Some(v) => encode_val(v, &mut payload)?,
                    None => payload.unit()?,
                }
                payload.finish()
            }
            Err(opt_err) => {
                let mut payload = writer.result_err()?;
                match opt_err {
                    Some(e) => encode_val(e, &mut payload)?,
                    None => payload.unit()?,
                }
                payload.finish()
            }
        },

        // ADTs - Variant (tagged union with optional payload)
        Val::Variant(tag, opt_payload) => {
            let mut payload = writer.variant(tag)?;
            match opt_payload {
                Some(v) => encode_val(v, &mut payload)?,
                None => payload.unit()?,
            }
            payload.finish()
        }

        // ADTs - Enum (variant with no payload)
        Val::Enum(tag) => {
            let mut payload = writer.variant(tag)?;
            payload.unit()?;
            payload.finish()
        }

        // Flags - encode as bitmap bytes
        // For a flags type with N flags, we need ceil(N/8) bytes
        // Each bit represents whether that flag is set
        Val::Flags(names) => {
            // Count total number of unique flag names to determine bitmap size
            // For now, encode the names list to determine indices
            // TODO: This requires the Type information to know the flag count
            // For now, fall back to simple encoding
            let mut list = writer.list()?;
            for name in names {
                list.str(name)?;
            }
            list.finish()
        }

        // Unsupported types
        Val::Resource(_) | Val::Future(_) | Val::Stream(_) | Val::ErrorContext(_) => {
            Err(Error::Malformed)
        }
    }
}
