//! # Flag Bitmap Encoding
//!
//! Efficient representation of flags as a bitmap packed into bytes.
//!
//! Instead of encoding flags as a list of strings (wasteful), we encode them
//! as a compact bitmap where each bit represents the presence/absence of a flag.
//!
//! ## Wire Format
//! - Tag: Bytes (0x11)
//! - Length: u32 (number of bytes needed for the bitmap)
//! - Data: Bitmap bytes (bit N set = flag N is active)

use crate::error::Result;
use crate::error::Error;

use neopack::Decoder;
use neopack::Encoder;

use wasmtime::component::Type;

/// Encodes a list of active flag names as a bitmap.
///
/// # Algorithm
/// 1. Iterate through all flag names defined in the type
/// 2. Set bit N if flag name N is in the active set
/// 3. Pack bits into bytes (little-endian bit order within each byte)
///
/// # Errors
/// Returns `RpcError::UnknownVariant` if an active flag is not defined in the type.
pub fn encode_flags_bitmap(enc: &mut Encoder, active: &[String], flags_handle: &Type) -> Result<()> {
    let Type::Flags(flags_ty) = flags_handle else {
        return Err(Error::ProtocolViolation("Expected Flags type".into()));
    };
    let all_names: Vec<&str> = flags_ty.names().collect();
    let num_flags = all_names.len();
    let num_bytes = (num_flags + 7) / 8;

    let mut bitmap = vec![0u8; num_bytes];

    for name in active {
        if let Some(idx) = all_names.iter().position(|n| n == name) {
            let byte_idx = idx / 8;
            let bit_idx = idx % 8;
            bitmap[byte_idx] |= 1 << bit_idx;
        } else {
            return Err(Error::UnknownVariant(name.clone()));
        }
    }

    enc.bytes(&bitmap)?;
    Ok(())
}

/// Decodes a bitmap into a list of active flag names.
///
/// # Algorithm
/// 1. Read the bitmap bytes
/// 2. For each bit that is set, add the corresponding flag name to the result
/// 3. Return the list in definition order
pub fn decode_flags_bitmap(dec: &mut Decoder, flags_handle: &Type) -> Result<Vec<String>> {
    let Type::Flags(flags_ty) = flags_handle else {
        return Err(Error::ProtocolViolation("Expected Flags type".into()));
    };
    let bitmap = dec.bytes()?;
    let all_names: Vec<&str> = flags_ty.names().collect();

    let mut active = Vec::new();

    for (idx, name) in all_names.iter().enumerate() {
        let byte_idx = idx / 8;
        let bit_idx = idx % 8;

        if byte_idx < bitmap.len() {
            if (bitmap[byte_idx] & (1 << bit_idx)) != 0 {
                active.push(name.to_string());
            }
        }
    }

    Ok(active)
}

#[cfg(test)]
mod tests {
    use super::*;

    use wasmtime::Engine;
    use wasmtime::component::Component;
    use wasmtime::component::Type;
    use wasmtime::component::types::ComponentItem;

    fn get_flags_type(flag_names: &[&str]) -> Type {
        let engine = Engine::default();
        let flags_list = flag_names.iter()
            .map(|n| format!(r#""{n}""#))
            .collect::<Vec<_>>()
            .join(" ");

        let wat = format!(r#"
            (component
                (type $f (flags {flags_list}))
                (export "f" (type $f))
            )
        "#);

        let component = Component::new(&engine, &wat).unwrap();
        let comp_ty = component.component_type();
        let exports: Vec<_> = comp_ty.exports(&engine).collect();

        let (_, item) = exports.iter().find(|(n, _)| *n == "f").unwrap();
        if let ComponentItem::Type(ty) = item {
            ty.clone()
        } else {
            panic!("Expected flags type");
        }
    }

    #[test]
    fn test_encode_decode_empty_flags() {
        let ft = get_flags_type(&["a", "b", "c"]);

        let mut enc = Encoder::new();
        encode_flags_bitmap(&mut enc, &[], &ft).unwrap();
        let bytes = enc.into_bytes().unwrap();

        let mut dec = Decoder::new(&bytes);
        let result = decode_flags_bitmap(&mut dec, &ft).unwrap();

        assert_eq!(result, Vec::<String>::new());
    }

    #[test]
    fn test_encode_decode_single_flag() {
        let ft = get_flags_type(&["read", "write", "execute"]);

        let mut enc = Encoder::new();
        encode_flags_bitmap(&mut enc, &["write".to_string()], &ft).unwrap();
        let bytes = enc.into_bytes().unwrap();

        let mut dec = Decoder::new(&bytes);
        let result = decode_flags_bitmap(&mut dec, &ft).unwrap();

        assert_eq!(result, vec!["write".to_string()]);
    }

    #[test]
    fn test_encode_decode_multiple_flags() {
        let ft = get_flags_type(&["a", "b", "c", "d", "e"]);

        let mut enc = Encoder::new();
        encode_flags_bitmap(&mut enc, &["a".to_string(), "c".to_string(), "e".to_string()], &ft).unwrap();
        let bytes = enc.into_bytes().unwrap();

        let mut dec = Decoder::new(&bytes);
        let result = decode_flags_bitmap(&mut dec, &ft).unwrap();

        assert_eq!(result, vec!["a".to_string(), "c".to_string(), "e".to_string()]);
    }

    #[test]
    fn test_encode_decode_all_flags() {
        let ft = get_flags_type(&["f1", "f2", "f3", "f4"]);

        let mut enc = Encoder::new();
        encode_flags_bitmap(&mut enc, &[
            "f1".to_string(),
            "f2".to_string(),
            "f3".to_string(),
            "f4".to_string()
        ], &ft).unwrap();
        let bytes = enc.into_bytes().unwrap();

        let mut dec = Decoder::new(&bytes);
        let result = decode_flags_bitmap(&mut dec, &ft).unwrap();

        assert_eq!(result, vec!["f1", "f2", "f3", "f4"]);
    }

    #[test]
    fn test_encode_many_flags_spanning_multiple_bytes() {
        let flags: Vec<&str> = (0..20).map(|i| {
            Box::leak(format!("flag{}", i).into_boxed_str()) as &str
        }).collect();

        let ft = get_flags_type(&flags);

        let active = vec![
            "flag0".to_string(),
            "flag7".to_string(),
            "flag8".to_string(),
            "flag15".to_string(),
            "flag19".to_string(),
        ];

        let mut enc = Encoder::new();
        encode_flags_bitmap(&mut enc, &active, &ft).unwrap();
        let bytes = enc.into_bytes().unwrap();

        let mut dec = Decoder::new(&bytes);
        let result = decode_flags_bitmap(&mut dec, &ft).unwrap();

        assert_eq!(result, active);
    }

    #[test]
    fn test_encode_unknown_flag_error() {
        let ft = get_flags_type(&["valid1", "valid2"]);

        let mut enc = Encoder::new();
        let result = encode_flags_bitmap(&mut enc, &["invalid".to_string()], &ft);

        match result {
            Err(Error::UnknownVariant(name)) => assert_eq!(name, "invalid"),
            _ => panic!("Expected UnknownVariant error"),
        }
    }

    #[test]
    fn test_bitmap_size_exactly_one_byte() {
        let ft = get_flags_type(&["a", "b", "c", "d", "e", "f", "g", "h"]);

        let mut enc = Encoder::new();
        encode_flags_bitmap(&mut enc, &["a".to_string(), "h".to_string()], &ft).unwrap();
        let bytes = enc.into_bytes().unwrap();

        let mut dec = Decoder::new(&bytes);
        let bitmap = dec.bytes().unwrap();

        assert_eq!(bitmap.len(), 1);
        assert_eq!(bitmap[0], 0b10000001);
    }

    #[test]
    fn test_bitmap_size_spans_two_bytes() {
        let ft = get_flags_type(&["a", "b", "c", "d", "e", "f", "g", "h", "i"]);

        let mut enc = Encoder::new();
        encode_flags_bitmap(&mut enc, &["a".to_string(), "i".to_string()], &ft).unwrap();
        let bytes = enc.into_bytes().unwrap();

        let mut dec = Decoder::new(&bytes);
        let bitmap = dec.bytes().unwrap();

        assert_eq!(bitmap.len(), 2);
        assert_eq!(bitmap[0], 0b00000001);
        assert_eq!(bitmap[1], 0b00000001);
    }

    #[test]
    fn test_out_of_order_flags_are_normalized() {
        let ft = get_flags_type(&["z", "y", "x", "w"]);

        let mut enc = Encoder::new();
        encode_flags_bitmap(&mut enc, &[
            "w".to_string(),
            "z".to_string(),
            "x".to_string(),
        ], &ft).unwrap();
        let bytes = enc.into_bytes().unwrap();

        let mut dec = Decoder::new(&bytes);
        let result = decode_flags_bitmap(&mut dec, &ft).unwrap();

        assert_eq!(result, vec!["z", "x", "w"]);
    }
}
