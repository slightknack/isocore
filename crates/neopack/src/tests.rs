use crate::*;
use std::f64::consts::PI;

// ============================================================================
//  SCALAR TESTS (Happy Path)
// ============================================================================

#[test]
fn test_bool_roundtrip() -> Result<()> {
    let mut enc = Encoder::new();
    enc.bool(true)?;
    enc.bool(false)?;

    let bytes = enc.into_bytes()?;
    let mut dec = Decoder::new(&bytes);

    assert_eq!(dec.bool()?, true);
    assert_eq!(dec.bool()?, false);
    assert_eq!(dec.remaining(), 0);
    Ok(())
}

#[test]
fn test_u8_s8_roundtrip() -> Result<()> {
    let mut enc = Encoder::new();
    enc.u8(0)?;
    enc.u8(255)?;
    enc.s8(127)?;
    enc.s8(-128)?;

    let bytes = enc.into_bytes()?;
    let mut dec = Decoder::new(&bytes);

    assert_eq!(dec.u8()?, 0);
    assert_eq!(dec.u8()?, 255);
    assert_eq!(dec.s8()?, 127);
    assert_eq!(dec.s8()?, -128);
    Ok(())
}

#[test]
fn test_u16_s16_roundtrip() -> Result<()> {
    let mut enc = Encoder::new();
    enc.u16(0)?;
    enc.u16(u16::MAX)?;
    enc.s16(i16::MAX)?;
    enc.s16(i16::MIN)?;

    let bytes = enc.into_bytes()?;
    let mut dec = Decoder::new(&bytes);

    assert_eq!(dec.u16()?, 0);
    assert_eq!(dec.u16()?, u16::MAX);
    assert_eq!(dec.s16()?, i16::MAX);
    assert_eq!(dec.s16()?, i16::MIN);
    Ok(())
}

#[test]
fn test_u32_s32_roundtrip() -> Result<()> {
    let mut enc = Encoder::new();
    enc.u32(0)?;
    enc.u32(u32::MAX)?;
    enc.s32(i32::MAX)?;
    enc.s32(i32::MIN)?;

    let bytes = enc.into_bytes()?;
    let mut dec = Decoder::new(&bytes);

    assert_eq!(dec.u32()?, 0);
    assert_eq!(dec.u32()?, u32::MAX);
    assert_eq!(dec.s32()?, i32::MAX);
    assert_eq!(dec.s32()?, i32::MIN);
    Ok(())
}

#[test]
fn test_u64_s64_roundtrip() -> Result<()> {
    let mut enc = Encoder::new();
    enc.u64(0)?;
    enc.u64(u64::MAX)?;
    enc.s64(i64::MAX)?;
    enc.s64(i64::MIN)?;

    let bytes = enc.into_bytes()?;
    let mut dec = Decoder::new(&bytes);

    assert_eq!(dec.u64()?, 0);
    assert_eq!(dec.u64()?, u64::MAX);
    assert_eq!(dec.s64()?, i64::MAX);
    assert_eq!(dec.s64()?, i64::MIN);
    Ok(())
}

#[test]
fn test_floats_roundtrip() -> Result<()> {
    let mut enc = Encoder::new();
    enc.f32(0.0)?;
    enc.f32(3.14159)?;
    enc.f64(0.0)?;
    enc.f64(PI)?;

    let bytes = enc.into_bytes()?;
    let mut dec = Decoder::new(&bytes);

    assert_eq!(dec.f32()?, 0.0);
    assert_eq!(dec.f32()?, 3.14159);
    assert_eq!(dec.f64()?, 0.0);
    assert_eq!(dec.f64()?, PI);
    Ok(())
}

#[test]
fn test_char_roundtrip() -> Result<()> {
    let mut enc = Encoder::new();
    enc.char('a')?;
    enc.char('🦀')?;

    let bytes = enc.into_bytes()?;
    let mut dec = Decoder::new(&bytes);

    assert_eq!(dec.char()?, 'a');
    assert_eq!(dec.char()?, '🦀');
    Ok(())
}

#[test]
fn test_unit_and_none() -> Result<()> {
    let mut enc = Encoder::new();
    enc.unit()?;
    enc.option_none()?;

    let bytes = enc.into_bytes()?;
    let mut dec = Decoder::new(&bytes);

    dec.unit()?;
    dec.option_none()?;
    assert_eq!(dec.remaining(), 0);
    Ok(())
}

// ============================================================================
//  BLOB TESTS (Happy Path)
// ============================================================================

#[test]
fn test_strings() -> Result<()> {
    let mut enc = Encoder::new();
    enc.str("hello")?;
    enc.str("")?;
    enc.str("❤️")?;

    let bytes = enc.into_bytes()?;
    let mut dec = Decoder::new(&bytes);

    assert_eq!(dec.str()?, "hello");
    assert_eq!(dec.str()?, "");
    assert_eq!(dec.str()?, "❤️");
    Ok(())
}

#[test]
fn test_bytes() -> Result<()> {
    let mut enc = Encoder::new();
    enc.bytes(&[1, 2, 3])?;
    enc.bytes(&[])?;

    let bytes = enc.into_bytes()?;
    let mut dec = Decoder::new(&bytes);

    assert_eq!(dec.bytes()?, &[1, 2, 3]);
    assert_eq!(dec.bytes()?, &[]);
    Ok(())
}

// ============================================================================
//  CONTAINER TESTS (Happy Path)
// ============================================================================

#[test]
fn test_list_simple() -> Result<()> {
    let mut enc = Encoder::new();
    enc.list_begin()?;
    enc.u32(1)?;
    enc.u32(2)?;
    enc.list_end()?;

    let bytes = enc.into_bytes()?;
    let mut dec = Decoder::new(&bytes);
    let mut list = dec.list()?;

    assert_eq!(list.next().unwrap().u32()?, 1);
    assert_eq!(list.next().unwrap().u32()?, 2);
    assert!(list.next().is_none());
    Ok(())
}

#[test]
fn test_list_nested() -> Result<()> {
    let mut enc = Encoder::new();
    enc.list_begin()?;
        enc.list_begin()?;
            enc.u32(10)?;
        enc.list_end()?;
        enc.u32(20)?;
    enc.list_end()?;

    let bytes = enc.into_bytes()?;
    let mut dec = Decoder::new(&bytes);
    let mut outer = dec.list()?;

    let mut inner = outer.next().unwrap().list()?;
    assert_eq!(inner.next().unwrap().u32()?, 10);

    assert_eq!(outer.next().unwrap().u32()?, 20);
    Ok(())
}

#[test]
fn test_map_logic() -> Result<()> {
    // Map of { "a": 1, "b": "two" }
    let mut enc = Encoder::new();
    enc.map_begin()?;
        enc.variant_begin("a")?;
            enc.u32(1)?;
        enc.variant_end()?;

        enc.variant_begin("b")?;
            enc.str("two")?;
        enc.variant_end()?;
    enc.map_end()?;

    let bytes = enc.into_bytes()?;
    let mut dec = Decoder::new(&bytes);
    let mut map = dec.map()?;

    let (k1, mut v1) = map.next()?.unwrap();
    assert_eq!(k1, "a");
    assert_eq!(v1.u32()?, 1);

    let (k2, mut v2) = map.next()?.unwrap();
    assert_eq!(k2, "b");
    assert_eq!(v2.str()?, "two");

    assert!(map.next()?.is_none());
    Ok(())
}

#[test]
fn test_map_empty() -> Result<()> {
    let mut enc = Encoder::new();
    enc.map_begin()?;
    enc.map_end()?;

    let bytes = enc.into_bytes()?;
    let mut dec = Decoder::new(&bytes);
    let mut map = dec.map()?;

    assert!(map.next()?.is_none());
    Ok(())
}

#[test]
fn test_option_some_workflow() -> Result<()> {
    let mut enc = Encoder::new();
    enc.option_some_begin()?;
    enc.u32(100)?;
    enc.option_some_end()?;

    let bytes = enc.into_bytes()?;
    let mut dec = Decoder::new(&bytes);

    let opt = dec.option()?;
    assert!(opt.is_some());
    assert_eq!(opt.unwrap().u32()?, 100);
    Ok(())
}

#[test]
fn test_result_ok_workflow() -> Result<()> {
    let mut enc = Encoder::new();
    enc.result_ok_begin()?;
    enc.str("ok")?;
    enc.result_ok_end()?;

    let bytes = enc.into_bytes()?;
    let mut dec = Decoder::new(&bytes);

    match dec.result()? {
        Ok(mut d) => assert_eq!(d.str()?, "ok"),
        Err(_) => panic!("Expected Ok"),
    }
    Ok(())
}

#[test]
fn test_result_err_workflow() -> Result<()> {
    let mut enc = Encoder::new();
    enc.result_err_begin()?;
    enc.u32(500)?;
    enc.result_err_end()?;

    let bytes = enc.into_bytes()?;
    let mut dec = Decoder::new(&bytes);

    match dec.result()? {
        Ok(_) => panic!("Expected Err"),
        Err(mut d) => assert_eq!(d.u32()?, 500),
    }
    Ok(())
}

#[test]
fn test_variant_workflow() -> Result<()> {
    let mut enc = Encoder::new();
    enc.variant_begin("MyEnum")?;
    enc.u32(123)?;
    enc.variant_end()?;

    let bytes = enc.into_bytes()?;
    let mut dec = Decoder::new(&bytes);

    let (name, mut val) = dec.variant()?;
    assert_eq!(name, "MyEnum");
    assert_eq!(val.u32()?, 123);
    Ok(())
}

// ============================================================================
//  COMPLEX INTEGRATION
// ============================================================================

#[test]
fn test_skip_logic() -> Result<()> {
    // Structure: List [ U32(1), Map(skipped), U32(2) ]
    let mut enc = Encoder::new();
    enc.list_begin()?;
        enc.u32(1)?;

        enc.map_begin()?;
            enc.variant_begin("key")?;
                enc.u32(99)?;
            enc.variant_end()?;
        enc.map_end()?;

        enc.u32(2)?;
    enc.list_end()?;

    let bytes = enc.into_bytes()?;
    let mut dec = Decoder::new(&bytes);
    let mut list = dec.list()?;

    // Read 1
    assert_eq!(list.next().unwrap().u32()?, 1);

    // Skip Map
    let mut map_decoder = list.next().unwrap();
    map_decoder.skip()?;

    // Read 2
    assert_eq!(list.next().unwrap().u32()?, 2);

    Ok(())
}

#[test]
fn test_deeply_nested_mixed() -> Result<()> {
    let mut enc = Encoder::new();
    enc.list_begin()?;
        enc.result_ok_begin()?;
            enc.option_some_begin()?;
                enc.variant_begin("Deep")?;
                    enc.u32(42)?;
                enc.variant_end()?;
            enc.option_some_end()?;
        enc.result_ok_end()?;
    enc.list_end()?;

    let bytes = enc.into_bytes()?;
    let mut dec = Decoder::new(&bytes);
    let mut list = dec.list()?;

    match list.next().unwrap().result()? {
        Ok(mut ok_dec) => {
            let mut opt_dec = ok_dec.option()?.unwrap();
            let (name, mut val) = opt_dec.variant()?;
            assert_eq!(name, "Deep");
            assert_eq!(val.u32()?, 42);
        },
        Err(_) => panic!("Expected Ok"),
    }
    Ok(())
}

// ============================================================================
//  ENCODER STRICTNESS FAILURE MODES
// ============================================================================

#[test]
fn test_strict_result_empty() {
    let mut enc = Encoder::new();
    enc.result_ok_begin().unwrap();
    // Error: Closed without writing a value
    match enc.result_ok_end() {
        Err(Error::EmptyAdt(Scope::Result)) => {},
        _ => panic!("Expected EmptyAdt error"),
    }
}

#[test]
fn test_strict_result_too_many() {
    let mut enc = Encoder::new();
    enc.result_ok_begin().unwrap();
    enc.u32(1).unwrap();
    // Error: Trying to write second value
    match enc.u32(2) {
        Err(Error::TooManyItems(Scope::Result)) => {},
        _ => panic!("Expected TooManyItems error"),
    }
}

#[test]
fn test_strict_option_empty() {
    let mut enc = Encoder::new();
    enc.option_some_begin().unwrap();
    match enc.option_some_end() {
        Err(Error::EmptyAdt(Scope::Option)) => {},
        _ => panic!("Expected EmptyAdt"),
    }
}

#[test]
fn test_strict_option_too_many() {
    let mut enc = Encoder::new();
    enc.option_some_begin().unwrap();
    enc.u32(1).unwrap();
    match enc.str("no") {
        Err(Error::TooManyItems(Scope::Option)) => {},
        _ => panic!("Expected TooManyItems"),
    }
}

#[test]
fn test_strict_variant_empty() {
    let mut enc = Encoder::new();
    enc.variant_begin("V").unwrap();
    // Note: The name "V" is metadata, not the payload item.
    match enc.variant_end() {
        Err(Error::EmptyAdt(Scope::Variant)) => {},
        _ => panic!("Expected EmptyAdt"),
    }
}

#[test]
fn test_strict_variant_too_many() {
    let mut enc = Encoder::new();
    enc.variant_begin("V").unwrap();
    enc.u32(1).unwrap();
    match enc.u32(2) {
        Err(Error::TooManyItems(Scope::Variant)) => {},
        _ => panic!("Expected TooManyItems"),
    }
}

#[test]
fn test_strict_map_entry_invalid_scalar() {
    let mut enc = Encoder::new();
    enc.map_begin().unwrap();
    // Error: Direct scalar in map
    match enc.u32(10) {
        Err(Error::InvalidMapEntry) => {},
        _ => panic!("Expected InvalidMapEntry"),
    }
}

#[test]
fn test_strict_map_entry_invalid_container() {
    let mut enc = Encoder::new();
    enc.map_begin().unwrap();
    // Error: List in map (must be Variant)
    match enc.list_begin() {
        Err(Error::InvalidMapEntry) => {},
        _ => panic!("Expected InvalidMapEntry"),
    }
}

// ============================================================================
//  ENCODER STATE ERRORS
// ============================================================================

#[test]
fn test_scope_mismatch() {
    let mut enc = Encoder::new();
    enc.list_begin().unwrap();
    match enc.map_end() {
        Err(Error::ScopeMismatch { expected, actual }) => {
            assert_eq!(expected, Scope::Map);
            assert_eq!(actual, Scope::List);
        },
        _ => panic!("Expected ScopeMismatch"),
    }
}

#[test]
fn test_scope_underflow() {
    let mut enc = Encoder::new();
    match enc.list_end() {
        Err(Error::ScopeUnderflow) => {},
        _ => panic!("Expected ScopeUnderflow"),
    }
}

#[test]
fn test_scope_still_open() {
    let mut enc = Encoder::new();
    enc.list_begin().unwrap();
    match enc.into_bytes() {
        Err(Error::ScopeStillOpen) => {},
        _ => panic!("Expected ScopeStillOpen"),
    }
}

// ============================================================================
//  DECODER FAILURE MODES
// ============================================================================

#[test]
fn test_fail_truncated_header() {
    let data = [0x10, 0x01]; // String tag + 1 byte (need 4 for len)
    let mut dec = Decoder::new(&data);
    match dec.str() {
        Err(Error::UnexpectedEnd) => {},
        _ => panic!("Expected UnexpectedEnd"),
    }
}

#[test]
fn test_fail_truncated_body() {
    let mut data = Vec::new();
    data.push(0x10); // String
    data.extend_from_slice(&100u32.to_le_bytes()); // Len 100
    data.push(0x01); // Only 1 byte of body

    let mut dec = Decoder::new(&data);
    match dec.str() {
        Err(Error::UnexpectedEnd) => {},
        _ => panic!("Expected UnexpectedEnd"),
    }
}

#[test]
fn test_fail_invalid_utf8_string() {
    let mut enc = Encoder::new();
    enc.bytes(&[0xFF, 0xFF]).unwrap();
    let mut raw = enc.into_bytes().unwrap();
    raw[0] = 0x10; // Patch Bytes tag to String tag

    let mut dec = Decoder::new(&raw);
    match dec.str() {
        Err(Error::InvalidUtf8) => {},
        _ => panic!("Expected InvalidUtf8"),
    }
}

#[test]
fn test_fail_invalid_utf8_variant_name() {
    // We want to simulate a Variant where the Name string contains invalid UTF-8.
    // Structure: [Variant Tag] [Body Len] [String Tag] [Name Len] [Invalid Byte] [Payload Tag]
    let mut packet = Vec::new();
    packet.push(0x33); // Variant Tag

    // Body Length:
    //   Tag::String (1) + Len (4) + Data (1) = 6 (The Name)
    //   Tag::Unit   (1)                      = 1 (The Payload)
    //   Total = 7
    packet.extend_from_slice(&7u32.to_le_bytes());
    packet.push(0x10); // String Tag
    packet.extend_from_slice(&1u32.to_le_bytes()); // Length of name
    packet.push(0xFF); // Invalid UTF-8 byte
    packet.push(0x0E);

    // Attempt to decode. It should parse the Tag/Len, enter the container,
    // and fail when attempting to convert the Name bytes to a string.
    let mut dec = Decoder::new(&packet);
    match dec.variant() {
        Err(Error::InvalidUtf8) => {},
        res => panic!("Expected InvalidUtf8, got {:?}", res),
    }
}

#[test]
fn test_fail_invalid_tag() {
    let data = [0xFF, 0x00];
    let dec = Decoder::new(&data);
    match dec.peek_tag() {
        Err(Error::InvalidTag(0xFF)) => {},
        _ => panic!("Expected InvalidTag"),
    }
}

#[test]
fn test_fail_map_non_variant_tag() {
    // Manually construct a Map containing a U32
    let mut enc = Encoder::new();
    enc.list_begin().unwrap();
    enc.u32(10).unwrap();
    enc.list_end().unwrap();

    let mut raw = enc.into_bytes().unwrap();
    raw[0] = 0x21; // Change List tag to Map tag

    let mut dec = Decoder::new(&raw);
    let mut map = dec.map().unwrap();

    match map.next() {
        Err(Error::InvalidTag(_)) => {},
        res => panic!("Expected InvalidTag error (maps require variants), got {:?}", res),
    }
}

#[test]
fn test_trailing_data_in_item_decoder() -> Result<()> {
    // Ensure that item decoders are strictly bounded
    let mut enc = Encoder::new();
    enc.list_begin()?;
    enc.u32(10)?;
    enc.list_end()?;

    let bytes = enc.into_bytes()?;
    let mut dec = Decoder::new(&bytes);
    let mut list = dec.list()?;

    let mut item_dec = list.next().unwrap();
    assert_eq!(item_dec.u32()?, 10);
    // Should be empty now
    assert!(item_dec.u32().is_err());
    Ok(())
}
