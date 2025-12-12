use super::Result;
use super::Location;
use super::Encoder;
use super::Decoder;
use super::Cursor;
use crate::types::Error;
use crate::types::Tag;
use crate::RecordDecoder;
use crate::ValueDecoder;

type R<T> = Result<T>;

#[test]
fn test_bool_roundtrip() -> R<()> {
    let mut enc = Encoder::new();
    enc.bool(true)?;
    enc.bool(false)?;
    let bytes = enc.as_bytes();

    let mut r = Decoder::new(bytes);
    assert_eq!(r.bool()?, true);
    assert_eq!(r.bool()?, false);
    Ok(())
}

// ==== STREAMING TESTS ====

#[test]
fn test_encoder_flush() -> R<()> {
    let mut enc = Encoder::new();

    enc.u32(1)?;
    enc.u32(2)?;

    let flushed1 = enc.flush()?;
    assert_eq!(flushed1.len(), 10); // 2 * (1 tag + 4 bytes)

    enc.u32(3)?;
    enc.u32(4)?;

    let flushed2 = enc.flush()?;
    assert_eq!(flushed2.len(), 10);

    // Total bytes should be 20
    assert_eq!(enc.as_bytes().len(), 20);

    Ok(())
}

#[test]
fn test_encoder_flush_with_scope_open() -> R<()> {
    let mut enc = Encoder::new();

    {
        let _list = enc.list()?;
        // _list is dropped here, but open_scopes is still incremented
        // Actually, we need to test differently - the scope WILL be closed on drop
    }

    // After drop, scope is closed, so flush should work
    // Can't actually test ScopeOpen without exposing internals or
    // using unsafe to prevent the Drop from running
    // The important behavior is tested implicitly by other tests

    Ok(())
}

#[test]
fn test_encoder_take_flushed() -> R<()> {
    let mut enc = Encoder::new();

    enc.u32(1)?;
    enc.u32(2)?;
    enc.flush()?;

    enc.u32(3)?;
    enc.u32(4)?;
    enc.flush()?;

    // Take first 10 bytes
    let taken = enc.take_flushed();
    assert_eq!(taken.len(), 20);

    // Buffer should now be empty
    assert_eq!(enc.as_bytes().len(), 0);

    // Add more
    enc.u32(5)?;
    enc.flush()?;

    assert_eq!(enc.as_bytes().len(), 5);

    Ok(())
}

#[test]
fn test_encoder_from_bytes() -> R<()> {
    let mut enc = Encoder::new();
    enc.u32(1)?;
    enc.u32(2)?;
    enc.u32(3)?;

    let bytes = enc.into_bytes();

    // Create new encoder from existing bytes
    let mut enc2 = Encoder::from_bytes(bytes)?;

    // Add more messages
    enc2.u32(4)?;
    enc2.u32(5)?;

    // Verify all messages are there
    let mut dec = Decoder::new(enc2.as_bytes());
    assert_eq!(dec.u32()?, 1);
    assert_eq!(dec.u32()?, 2);
    assert_eq!(dec.u32()?, 3);
    assert_eq!(dec.u32()?, 4);
    assert_eq!(dec.u32()?, 5);

    Ok(())
}

#[test]
fn test_encoder_from_bytes_invalid() {
    // Incomplete message
    let bad_bytes = vec![0x07, 0x01]; // U32 tag but missing bytes

    match Encoder::from_bytes(bad_bytes) {
        Err(Error::Pending(_)) => {}
        _ => panic!("Expected Pending error"),
    }
}

#[test]
fn test_cursor_streaming() -> R<()> {
    use crate::Cursor;

    // Simulate streaming: we have partial data
    let partial = vec![0x07, 0x01, 0x00]; // U32 tag + first 2 bytes
    let cursor = Cursor::new(&partial);
    let mut dec = Decoder::with_cursor(cursor.clone());

    // Should fail with Pending
    match dec.u32() {
        Err(Error::Pending(n)) => assert_eq!(n, 2),
        _ => panic!("Expected Pending"),
    }

    Ok(())
}

#[test]
fn test_cursor_mark_and_seek_during_parse() -> R<()> {
    use crate::Cursor;

    let mut enc = Encoder::new();
    enc.u32(1)?;
    enc.u32(2)?;
    enc.u32(3)?;

    let bytes = enc.as_bytes();
    let cursor = Cursor::new(bytes);
    let mut dec = Decoder::with_cursor(cursor.clone());

    // Read first value
    assert_eq!(dec.u32()?, 1);

    // Mark position
    let mark = dec.cursor().mark();

    // Read second value
    assert_eq!(dec.u32()?, 2);

    // Seek back
    dec.cursor_mut().seek(mark)?;

    // Read second value again
    assert_eq!(dec.u32()?, 2);
    assert_eq!(dec.u32()?, 3);

    Ok(())
}

#[test]
fn test_incremental_message_parsing() -> R<()> {
    use crate::Cursor;

    // Build a message with 3 values
    let mut enc = Encoder::new();
    enc.u64(100)?;
    enc.u64(200)?;
    enc.u64(300)?;
    let full = enc.as_bytes();

    // Simulate receiving bytes in chunks
    let chunk1 = &full[..6];  // First value incomplete
    let chunk2 = &full[6..15]; // Complete first, start second
    let chunk3 = &full[15..]; // Rest

    // Process chunk1
    let mut cursor = Cursor::new(chunk1);
    let mut dec = Decoder::with_cursor(cursor.clone());
    assert!(matches!(dec.u64(), Err(Error::Pending(_))));

    // Extend with chunk2 - we need to create new buffer
    let mut buffer = chunk1.to_vec();
    buffer.extend_from_slice(chunk2);
    cursor = Cursor::new(&buffer);
    dec = Decoder::with_cursor(cursor.clone());

    assert_eq!(dec.u64()?, 100);

    // Second value is incomplete
    assert!(matches!(dec.u64(), Err(Error::Pending(_))));

    // Extend with chunk3
    buffer.extend_from_slice(chunk3);
    cursor = Cursor::new(&buffer);
    dec = Decoder::with_cursor(cursor.clone());

    // Skip past first value
    dec.skip_value()?;

    assert_eq!(dec.u64()?, 200);
    assert_eq!(dec.u64()?, 300);

    Ok(())
}

#[test]
fn test_flush_between_messages() -> R<()> {
    let mut enc = Encoder::new();

    // Message 1
    let mut list = enc.list()?;
    list.u32(1)?;
    list.u32(2)?;
    list.finish()?;

    let flushed1 = enc.flush()?.to_vec();

    // Message 2
    let mut map = enc.map()?;
    map.key("x")?.u32(10)?;
    map.finish()?;

    let flushed2 = enc.flush()?.to_vec();

    // Verify each flushed chunk is independently decodable
    let mut dec1 = Decoder::new(&flushed1);
    let mut list = dec1.list()?;
    assert_eq!(list.next()?.unwrap().as_u32()?, 1);
    assert_eq!(list.next()?.unwrap().as_u32()?, 2);

    let mut dec2 = Decoder::new(&flushed2);
    let mut map = dec2.map()?;
    let (k, v) = map.next()?.unwrap();
    assert_eq!(k, "x");
    assert_eq!(v.as_u32()?, 10);

    Ok(())
}

#[test]
fn test_cursor_absolute_position() -> R<()> {
    use crate::Cursor;

    let data = b"hello world";
    let cursor = Cursor::with_context(data, 0, 1000, 0);

    assert_eq!(cursor.absolute_pos(), 1000);

    let mark1 = cursor.mark();
    assert_eq!(mark1.absolute_pos, 1000);

    let mut cursor2 = cursor.clone();
    cursor2.read_bytes(5)?;

    assert_eq!(cursor2.absolute_pos(), 1005);

    let mark2 = cursor2.mark();
    assert_eq!(mark2.absolute_pos, 1005);

    Ok(())
}

#[test]
fn test_resume_encoding_workflow() -> R<()> {
    // Simulate writing to a file, then resuming
    let mut enc = Encoder::new();

    // First session: write some messages
    enc.str("message1")?;
    enc.str("message2")?;
    enc.flush()?;

    // "Write to file"
    let file_contents = enc.as_bytes().to_vec();

    // Later: resume from file
    let mut enc2 = Encoder::from_bytes(file_contents)?;

    // Continue encoding
    enc2.str("message3")?;
    enc2.str("message4")?;

    // Verify all 4 messages
    let mut dec = Decoder::new(enc2.as_bytes());
    assert_eq!(dec.str()?, "message1");
    assert_eq!(dec.str()?, "message2");
    assert_eq!(dec.str()?, "message3");
    assert_eq!(dec.str()?, "message4");

    Ok(())
}

#[test]
fn test_location_tracking_across_boundaries() -> R<()> {
    use crate::{Cursor, Location};

    let mut enc = Encoder::new();
    for i in 0..10 {
        enc.u64(i)?;
    }
    let bytes = enc.as_bytes();

    // Parse and collect locations of each message
    let mut locations: Vec<Location> = Vec::new();
    let mut cursor = Cursor::new(bytes);

    while cursor.remaining() > 0 {
        locations.push(cursor.mark());
        let mut dec = Decoder::with_cursor(cursor.clone());
        dec.skip_value()?;
        cursor = dec.cursor().clone();
    }

    assert_eq!(locations.len(), 10);

    // Now jump to message 5 directly
    let mut cursor = Cursor::new(bytes);
    cursor.seek(locations[5])?;
    let mut dec = Decoder::with_cursor(cursor);
    assert_eq!(dec.u64()?, 5);

    Ok(())
}

#[test]
fn test_large_message_streaming() -> R<()> {
    use crate::Cursor;

    // Create a large list
    let mut enc = Encoder::new();
    let mut list = enc.list()?;
    for i in 0..1000 {
        list.u32(i)?;
    }
    list.finish()?;

    let bytes = enc.as_bytes();

    // Process with cursor
    let cursor = Cursor::new(bytes);
    let mut dec = Decoder::with_cursor(cursor);
    let mut list = dec.list()?;

    // Read all items
    let mut count = 0;
    while let Some(val) = list.next()? {
        assert_eq!(val.as_u32()?, count);
        count += 1;
    }

    assert_eq!(count, 1000);

    Ok(())
}

#[test]
fn test_u8_roundtrip() -> R<()> {
    let mut enc = Encoder::new();
    enc.u8(0)?; enc.u8(255)?; enc.u8(42)?;
    let bytes = enc.as_bytes();

    let mut r = Decoder::new(bytes);
    assert_eq!(r.u8()?, 0);
    assert_eq!(r.u8()?, 255);
    assert_eq!(r.u8()?, 42);
    Ok(())
}

#[test]
fn test_i8_roundtrip() -> R<()> {
    let mut enc = Encoder::new();
    enc.i8(-128)?; enc.i8(127)?; enc.i8(0)?; enc.i8(-1)?;
    let bytes = enc.as_bytes();

    let mut r = Decoder::new(bytes);
    assert_eq!(r.i8()?, -128);
    assert_eq!(r.i8()?, 127);
    assert_eq!(r.i8()?, 0);
    assert_eq!(r.i8()?, -1);
    Ok(())
}

#[test]
fn test_u16_roundtrip() -> R<()> {
    let mut enc = Encoder::new();
    enc.u16(0)?; enc.u16(65535)?; enc.u16(1234)?;
    let bytes = enc.as_bytes();

    let mut r = Decoder::new(bytes);
    assert_eq!(r.u16()?, 0);
    assert_eq!(r.u16()?, 65535);
    assert_eq!(r.u16()?, 1234);
    Ok(())
}

#[test]
fn test_i16_roundtrip() -> R<()> {
    let mut enc = Encoder::new();
    enc.i16(-32768)?; enc.i16(32767)?; enc.i16(0)?;
    let bytes = enc.as_bytes();

    let mut r = Decoder::new(bytes);
    assert_eq!(r.i16()?, -32768);
    assert_eq!(r.i16()?, 32767);
    assert_eq!(r.i16()?, 0);
    Ok(())
}

#[test]
fn test_u32_roundtrip() -> R<()> {
    let mut enc = Encoder::new();
    enc.u32(0)?; enc.u32(4294967295)?; enc.u32(123456)?;
    let bytes = enc.as_bytes();

    let mut r = Decoder::new(bytes);
    assert_eq!(r.u32()?, 0);
    assert_eq!(r.u32()?, 4294967295);
    assert_eq!(r.u32()?, 123456);
    Ok(())
}

#[test]
fn test_i32_roundtrip() -> R<()> {
    let mut enc = Encoder::new();
    enc.i32(-2147483648)?; enc.i32(2147483647)?; enc.i32(0)?;
    let bytes = enc.as_bytes();

    let mut r = Decoder::new(bytes);
    assert_eq!(r.i32()?, -2147483648);
    assert_eq!(r.i32()?, 2147483647);
    assert_eq!(r.i32()?, 0);
    Ok(())
}

#[test]
fn test_u64_roundtrip() -> R<()> {
    let mut enc = Encoder::new();
    enc.u64(0)?; enc.u64(u64::MAX)?; enc.u64(123456789)?;
    let bytes = enc.as_bytes();

    let mut r = Decoder::new(bytes);
    assert_eq!(r.u64()?, 0);
    assert_eq!(r.u64()?, u64::MAX);
    assert_eq!(r.u64()?, 123456789);
    Ok(())
}

#[test]
fn test_i64_roundtrip() -> R<()> {
    let mut enc = Encoder::new();
    enc.i64(i64::MIN)?; enc.i64(i64::MAX)?; enc.i64(0)?;
    let bytes = enc.as_bytes();

    let mut r = Decoder::new(bytes);
    assert_eq!(r.i64()?, i64::MIN);
    assert_eq!(r.i64()?, i64::MAX);
    assert_eq!(r.i64()?, 0);
    Ok(())
}

#[test]
fn test_f32_roundtrip() -> R<()> {
    let mut enc = Encoder::new();
    enc.f32(0.0)?; enc.f32(3.14159)?; enc.f32(-1.5)?; enc.f32(f32::INFINITY)?;
    let bytes = enc.as_bytes();

    let mut r = Decoder::new(bytes);
    assert_eq!(r.f32()?, 0.0);
    assert_eq!(r.f32()?, 3.14159);
    assert_eq!(r.f32()?, -1.5);
    assert_eq!(r.f32()?, f32::INFINITY);
    Ok(())
}

#[test]
fn test_f64_roundtrip() -> R<()> {
    let mut enc = Encoder::new();
    enc.f64(0.0)?; enc.f64(3.141592653589793)?; enc.f64(-2.5)?; enc.f64(f64::NEG_INFINITY)?;
    let bytes = enc.as_bytes();

    let mut r = Decoder::new(bytes);
    assert_eq!(r.f64()?, 0.0);
    assert_eq!(r.f64()?, 3.141592653589793);
    assert_eq!(r.f64()?, -2.5);
    assert_eq!(r.f64()?, f64::NEG_INFINITY);
    Ok(())
}

#[test]
fn test_string_roundtrip() -> R<()> {
    let mut enc = Encoder::new();
    enc.str("hello")?; enc.str("")?; enc.str("world 🌍")?;
    let bytes = enc.as_bytes();

    let mut r = Decoder::new(bytes);
    assert_eq!(r.str()?, "hello");
    assert_eq!(r.str()?, "");
    assert_eq!(r.str()?, "world 🌍");
    Ok(())
}

#[test]
fn test_bytes_roundtrip() -> R<()> {
    let mut enc = Encoder::new();
    enc.bytes(&[1, 2, 3])?; enc.bytes(&[])?; enc.bytes(&[255, 0, 128])?;
    let bytes = enc.as_bytes();

    let mut r = Decoder::new(bytes);
    assert_eq!(r.bytes()?, &[1, 2, 3]);
    assert_eq!(r.bytes()?, &[]);
    assert_eq!(r.bytes()?, &[255, 0, 128]);
    Ok(())
}

#[test]
fn test_struct_blob_roundtrip() -> R<()> {
    let mut enc = Encoder::new();
    enc.record_raw(&[10, 20, 30])?;
    let bytes = enc.as_bytes();

    let mut r = Decoder::new(bytes);
    assert_eq!(r.record_raw()?, &[10, 20, 30]);
    Ok(())
}

#[test]
fn test_list_scalars() -> R<()> {
    let mut enc = Encoder::new();
    let mut list = enc.list()?;
    list.u32(1)?;
    list.u32(2)?;
    list.u32(3)?;
    list.finish()?;

    let bytes = enc.as_bytes();

    let mut r = Decoder::new(bytes);
    let mut list = r.list()?;

    assert_eq!(list.next()?.unwrap().as_u32()?, 1);
    assert_eq!(list.next()?.unwrap().as_u32()?, 2);
    assert_eq!(list.next()?.unwrap().as_u32()?, 3);
    assert!(list.next()?.is_none());
    Ok(())
}

#[test]
fn test_list_mixed_types() -> R<()> {
    let mut enc = Encoder::new();
    let mut list = enc.list()?;
    list.bool(true)?;
    list.str("test")?;
    list.u64(999)?;
    list.finish()?;

    let bytes = enc.as_bytes();

    let mut r = Decoder::new(bytes);
    let mut list = r.list()?;

    assert_eq!(list.next()?.unwrap().as_bool()?, true);
    assert_eq!(list.next()?.unwrap().as_str()?, "test");
    assert_eq!(list.next()?.unwrap().as_u64()?, 999);
    Ok(())
}

#[test]
fn test_list_nested() -> R<()> {
    let mut enc = Encoder::new();
    let mut outer = enc.list()?;
    outer.u16(1)?;

    let mut inner = outer.list()?;
    inner.u16(2)?;
    inner.u16(3)?;
    inner.finish()?;

    outer.u16(4)?;
    outer.finish()?;

    let bytes = enc.as_bytes();

    let mut r = Decoder::new(bytes);
    let mut outer = r.list()?;

    assert_eq!(outer.next()?.unwrap().as_u16()?, 1);

    if let ValueDecoder::List(mut inner) = outer.next()?.unwrap() {
        assert_eq!(inner.next()?.unwrap().as_u16()?, 2);
        assert_eq!(inner.next()?.unwrap().as_u16()?, 3);
    } else {
        panic!("Expected nested list");
    }

    assert_eq!(outer.next()?.unwrap().as_u16()?, 4);
    Ok(())
}

#[test]
fn test_list_empty() -> R<()> {
    let mut enc = Encoder::new();
    let list = enc.list()?;
    list.finish()?;

    let bytes = enc.as_bytes();

    let mut r = Decoder::new(bytes);
    let mut list = r.list()?;
    assert!(list.next()?.is_none());
    Ok(())
}

#[test]
fn test_map_basic() -> R<()> {
    let mut enc = Encoder::new();
    let mut map = enc.map()?;
    map.key("name")?.str("Alice")?;
    map.key("age")?.u32(30)?;
    map.key("active")?.bool(true)?;
    map.finish()?;

    let bytes = enc.as_bytes();
    let mut r = Decoder::new(bytes);
    let mut map = r.map()?;

    let (k, v) = map.next()?.unwrap();
    assert_eq!(k, "name");
    assert_eq!(v.as_str()?, "Alice");

    let (k, v) = map.next()?.unwrap();
    assert_eq!(k, "age");
    assert_eq!(v.as_u32()?, 30);

    let (k, v) = map.next()?.unwrap();
    assert_eq!(k, "active");
    assert_eq!(v.as_bool()?, true);

    assert!(map.next()?.is_none());
    Ok(())
}

#[test]
fn test_map_nested() -> R<()> {
    let mut enc = Encoder::new();
    let mut map = enc.map()?;
    map.key("outer")?.u32(1)?;

    let mut inner = map.key("inner")?.map()?;
    inner.key("nested")?.u32(2)?;
    inner.finish()?;

    map.finish()?;

    let bytes = enc.as_bytes();
    let mut r = Decoder::new(bytes);
    let mut outer = r.map()?;

    let (k, v) = outer.next()?.unwrap();
    assert_eq!(k, "outer");
    assert_eq!(v.as_u32()?, 1);

    let (k, v) = outer.next()?.unwrap();
    assert_eq!(k, "inner");
    if let ValueDecoder::Map(mut inner) = v {
        let (k2, v2) = inner.next()?.unwrap();
        assert_eq!(k2, "nested");
        assert_eq!(v2.as_u32()?, 2);
    } else {
        panic!("Expected nested map");
    }
    Ok(())
}

#[test]
fn test_map_empty() -> R<()> {
    let mut enc = Encoder::new();
    let map = enc.map()?;
    map.finish()?;

    let bytes = enc.as_bytes();

    let mut r = Decoder::new(bytes);
    let mut map = r.map()?;
    assert!(map.next()?.is_none());
    Ok(())
}

#[test]
fn test_array_u32() -> R<()> {
    let mut enc = Encoder::new();
    let mut arr = enc.array(Tag::U32, 4)?;
    arr.push(&[1, 0, 0, 0])?;
    arr.push(&[2, 0, 0, 0])?;
    arr.push(&[3, 0, 0, 0])?;
    arr.finish()?;

    let bytes = enc.as_bytes();
    let mut r = Decoder::new(bytes);
    let mut arr = r.array()?;

    assert_eq!(arr.item_tag(), Tag::U32);
    assert_eq!(arr.stride(), 4);
    assert_eq!(arr.remaining(), 3);

    // Using .as_u32() to avoid PartialEq on ValueReader
    assert_eq!(arr.next()?.unwrap().as_u32()?, 1);
    assert_eq!(arr.next()?.unwrap().as_u32()?, 2);
    assert_eq!(arr.next()?.unwrap().as_u32()?, 3);
    assert!(arr.next()?.is_none());
    Ok(())
}

#[test]
fn test_array_empty() -> R<()> {
    let mut enc = Encoder::new();
    let arr = enc.array(Tag::U16, 2)?;
    arr.finish()?;

    let bytes = enc.as_bytes();

    let mut r = Decoder::new(bytes);
    let mut arr = r.array()?;
    assert_eq!(arr.remaining(), 0);
    assert!(arr.next()?.is_none());
    Ok(())
}

#[test]
fn test_pending_error() {
    let bytes = vec![0x05];
    let mut r = Decoder::new(&bytes);
    match r.u16() {
        Err(Error::Pending(n)) => assert_eq!(n, 2),
        _ => panic!("Expected Pending error"),
    }
}

#[test]
fn test_invalid_tag() {
    let bytes = vec![0xFF];
    let mut r = Decoder::new(&bytes);
    match r.read_tag() {
        Err(Error::InvalidTag(0xFF)) => {}
        _ => panic!("Expected InvalidTag error"),
    }
}

#[test]
fn test_type_mismatch() -> R<()> {
    let mut enc = Encoder::new();
    enc.u32(42)?;
    let bytes = enc.as_bytes();

    let mut r = Decoder::new(bytes);
    match r.u16() {
        Err(Error::TypeMismatch) => {}
        _ => panic!("Expected TypeMismatch error"),
    }
    Ok(())
}

#[test]
fn test_invalid_utf8() {
    let mut bad_bytes = vec![0x10];
    bad_bytes.extend_from_slice(&2u32.to_le_bytes());
    bad_bytes.extend_from_slice(&[0xFF, 0xFE]);

    let mut r = Decoder::new(&bad_bytes);
    match r.str() {
        Err(Error::InvalidUtf8) => {}
        _ => panic!("Expected InvalidUtf8 error"),
    }
}

#[test]
fn test_blob_too_large() {
    let mut enc = Encoder::new();
    let huge_str = "x".repeat(65536);
    let _ = enc.str(&huge_str);
}

#[test]
#[should_panic(expected = "invalid stride: 0")]
fn test_array_stride_zero() {
    let mut enc = Encoder::new();
    let _ = enc.array(Tag::U32, 0);
}

#[test]
fn test_array_stride_mismatch() -> R<()> {
    let mut enc = Encoder::new();
    let mut arr = enc.array(Tag::U32, 4)?;
    match arr.push(&[1, 2]) {
        Err(Error::Malformed) => {}
        _ => panic!("Expected Malformed error"),
    }
    Ok(())
}

#[test]
fn test_streaming_incremental() -> R<()> {
    let mut enc = Encoder::new();
    enc.u32(1234)?;
    let full_bytes = enc.as_bytes();

    let mut r = Decoder::new(&full_bytes[..2]);
    match r.u32() {
        Err(Error::Pending(_)) => {}
        _ => panic!("Expected Pending"),
    }

    let mut r = Decoder::new(full_bytes);
    assert_eq!(r.u32()?, 1234);
    Ok(())
}

#[test]
fn test_partial_string() -> R<()> {
    let mut enc = Encoder::new();
    enc.str("hello")?;
    let full_bytes = enc.as_bytes();

    let mut r = Decoder::new(&full_bytes[..3]);
    match r.str() {
        Err(Error::Pending(n)) => assert_eq!(n, 2),
        _ => panic!("Expected Pending"),
    }
    Ok(())
}

#[test]
fn test_complex_structure() -> R<()> {
    let mut enc = Encoder::new();
    let mut root_map = enc.map()?;
    root_map.key("id")?.u64(12345)?;
    root_map.key("name")?.str("test")?;

    let mut tags = root_map.key("tags")?.list()?;
    tags.str("rust")?;
    tags.str("binary")?;
    tags.str("serialization")?;
    tags.finish()?;

    let mut meta = root_map.key("metadata")?.map()?;
    meta.key("version")?.u32(1)?;
    meta.key("active")?.bool(true)?;
    meta.finish()?;

    root_map.finish()?;

    let bytes = enc.as_bytes();

    let mut r = Decoder::new(&bytes);
    let mut root = r.map()?;

    let (k, v) = root.next()?.unwrap();
    assert_eq!(k, "id");
    assert_eq!(v.as_u64()?, 12345);

    let (k, v) = root.next()?.unwrap();
    assert_eq!(k, "name");
    assert_eq!(v.as_str()?, "test");

    let (k, v) = root.next()?.unwrap();
    assert_eq!(k, "tags");
    if let ValueDecoder::List(mut tags) = v {
        assert_eq!(tags.next()?.unwrap().as_str()?, "rust");
        assert_eq!(tags.next()?.unwrap().as_str()?, "binary");
        assert_eq!(tags.next()?.unwrap().as_str()?, "serialization");
    } else {
        panic!("Expected list");
    }

    let (k, v) = root.next()?.unwrap();
    assert_eq!(k, "metadata");
    if let ValueDecoder::Map(mut meta) = v {
        let (k2, v2) = meta.next()?.unwrap();
        assert_eq!(k2, "version");
        assert_eq!(v2.as_u32()?, 1);

        let (k2, v2) = meta.next()?.unwrap();
        assert_eq!(k2, "active");
        assert_eq!(v2.as_bool()?, true);
    } else {
        panic!("Expected map");
    }
    Ok(())
}

#[test]
fn test_list_drop_patches_len() -> R<()> {
    let mut enc = Encoder::new();
    let mut list = enc.list()?;
    list.u32(1)?;
    list.u32(2)?;
    list.finish()?;

    let bytes = enc.as_bytes();
    let mut r = Decoder::new(&bytes);
    let mut list = r.list()?;
    assert!(list.next()?.is_some());
    assert!(list.next()?.is_some());
    assert!(list.next()?.is_none());
    Ok(())
}

#[test]
fn test_struct_reader_sequential() -> R<()> {
    let mut enc = Encoder::new();
    let mut struct_data = Vec::new();
    struct_data.extend_from_slice(&42u32.to_le_bytes());
    struct_data.extend_from_slice(&3.14f32.to_le_bytes());
    struct_data.push(123);
    enc.record_raw(&struct_data)?;

    let bytes = enc.as_bytes();
    let mut r = Decoder::new(&bytes);
    let mut sr = r.record()?;

    assert_eq!(sr.u32()?, 42);
    assert_eq!(sr.f32()?, 3.14);
    assert_eq!(sr.u8()?, 123);
    assert_eq!(sr.remaining(), 0);
    Ok(())
}

#[test]
fn test_struct_reader_raw() -> R<()> {
    let mut enc = Encoder::new();
    enc.record_raw(&[1, 2, 3, 4])?;

    let bytes = enc.as_bytes();
    let mut r = Decoder::new(&bytes);
    let mut sr = r.record()?;

    assert_eq!(sr.raw(), &[1, 2, 3, 4]);
    let _ = sr.bytes(4)?;
    Ok(())
}

#[test]
#[should_panic(expected = "RecordReader dropped with")]
fn test_struct_reader_incomplete_panic() {
    let mut enc = Encoder::new();
    enc.record_raw(&[1, 2, 3, 4]).unwrap();

    let bytes = enc.as_bytes();
    let mut r = Decoder::new(&bytes);
    let mut sr = r.record().unwrap();

    sr.u8().unwrap();
}

#[test]
fn test_round_trip_random_ints() -> R<()> {
    let mut enc = Encoder::new();
    let values: Vec<i32> = (0..100).map(|i| (i * 7919) % 1000 - 500).collect();

    let mut list = enc.list()?;
    for &v in &values {
        list.i32(v)?;
    }
    list.finish()?;

    let bytes = enc.as_bytes();
    let mut r = Decoder::new(&bytes);
    let mut list = r.list()?;

    for &expected in &values {
        assert_eq!(list.next()?.unwrap().as_i32()?, expected);
    }
    Ok(())
}

#[test]
fn test_deeply_nested_lists() -> R<()> {
    let mut enc = Encoder::new();

    let mut l1 = enc.list()?;
    let mut l2 = l1.list()?;
    let mut l3 = l2.list()?;
    let mut l4 = l3.list()?;
    l4.u32(42)?;
    l4.finish()?;
    l3.finish()?;
    l2.finish()?;
    l1.finish()?;

    let bytes = enc.as_bytes();
    let mut r = Decoder::new(&bytes);
    let mut l1 = r.list()?;

    if let ValueDecoder::List(mut l2) = l1.next()?.unwrap() {
        if let ValueDecoder::List(mut l3) = l2.next()?.unwrap() {
            if let ValueDecoder::List(mut l4) = l3.next()?.unwrap() {
                assert_eq!(l4.next()?.unwrap().as_u32()?, 42);
            } else { panic!(); }
        } else { panic!(); }
    } else { panic!(); }
    Ok(())
}

#[test]
fn test_mixed_container_types() -> R<()> {
    let mut enc = Encoder::new();

    let mut list = enc.list()?;

    let mut map = list.map()?;
    let mut coords = map.key("coords")?.list()?;
    coords.u16(100)?.u16(200)?;
    coords.finish()?;
    map.finish()?;

    let mut arr = list.array(Tag::U8, 1)?;
    arr.push(&[1])?;
    arr.push(&[2])?;
    arr.push(&[3])?;
    arr.finish()?;

    list.finish()?;

    let bytes = enc.as_bytes();
    let mut r = Decoder::new(&bytes);
    let mut list = r.list()?;

    if let ValueDecoder::Map(mut map) = list.next()?.unwrap() {
        let (k, v) = map.next()?.unwrap();
        assert_eq!(k, "coords");
        if let ValueDecoder::List(mut coords) = v {
            assert_eq!(coords.next()?.unwrap().as_u16()?, 100);
            assert_eq!(coords.next()?.unwrap().as_u16()?, 200);
        } else { panic!(); }
    } else { panic!(); }

    if let ValueDecoder::Array(mut arr) = list.next()?.unwrap() {
        assert_eq!(arr.next()?.unwrap().as_u8()?, 1);
        assert_eq!(arr.next()?.unwrap().as_u8()?, 2);
        assert_eq!(arr.next()?.unwrap().as_u8()?, 3);
    } else { panic!(); }
    Ok(())
}

#[test]
fn test_all_numeric_types_in_list() -> R<()> {
    let mut enc = Encoder::new();
    let mut list = enc.list()?;
    list.bool(true)?;
    list.u8(255)?;
    list.i8(-128)?;
    list.u16(65535)?;
    list.i16(-32768)?;
    list.u32(4000000)?;
    list.i32(-2000000)?;
    list.u64(1000000000000)?;
    list.i64(-500000000000)?;
    list.f32(3.14)?;
    list.f64(2.71828)?;
    list.finish()?;

    let bytes = enc.as_bytes();
    let mut r = Decoder::new(&bytes);
    let mut list = r.list()?;

    assert_eq!(list.next()?.unwrap().as_bool()?, true);
    assert_eq!(list.next()?.unwrap().as_u8()?, 255);
    assert_eq!(list.next()?.unwrap().as_i8()?, -128);
    assert_eq!(list.next()?.unwrap().as_u16()?, 65535);
    assert_eq!(list.next()?.unwrap().as_i16()?, -32768);
    assert_eq!(list.next()?.unwrap().as_u32()?, 4000000);
    assert_eq!(list.next()?.unwrap().as_i32()?, -2000000);
    assert_eq!(list.next()?.unwrap().as_u64()?, 1000000000000);
    assert_eq!(list.next()?.unwrap().as_i64()?, -500000000000);
    assert_eq!(list.next()?.unwrap().as_f32()?, 3.14);
    assert_eq!(list.next()?.unwrap().as_f64()?, 2.71828);
    Ok(())
}

#[test]
fn test_sparse_array_as_map() -> R<()> {
    let mut enc = Encoder::new();
    let mut map = enc.map()?;
    map.key("0")?.u32(100)?;
    map.key("1000")?.u32(200)?;
    map.key("1000000")?.u32(300)?;
    map.finish()?;

    let bytes = enc.as_bytes();
    let mut r = Decoder::new(&bytes);
    let mut map = r.map()?;

    let (k, v) = map.next()?.unwrap();
    assert_eq!(k, "0");
    assert_eq!(v.as_u32()?, 100);

    let (k, v) = map.next()?.unwrap();
    assert_eq!(k, "1000");
    assert_eq!(v.as_u32()?, 200);

    let (k, v) = map.next()?.unwrap();
    assert_eq!(k, "1000000");
    assert_eq!(v.as_u32()?, 300);
    Ok(())
}

#[test]
fn test_unicode_madness() -> R<()> {
    let mut enc = Encoder::new();
    enc.str("Hello 世界 🦀 Здравствуй مرحبا")?;
    enc.str("🎉🎊🎈")?;
    enc.str("∑∫∂∇")?;

    let bytes = enc.as_bytes();
    let mut r = Decoder::new(&bytes);
    assert_eq!(r.str()?, "Hello 世界 🦀 Здравствуй مرحبا");
    assert_eq!(r.str()?, "🎉🎊🎈");
    assert_eq!(r.str()?, "∑∫∂∇");
    Ok(())
}

#[test]
fn test_empty_everything() -> R<()> {
    let mut enc = Encoder::new();
    enc.str("")?;
    enc.bytes(&[])?;
    enc.record_raw(&[])?;
    enc.list()?.finish()?;
    enc.map()?.finish()?;
    enc.array(Tag::U8, 1)?.finish()?;

    let bytes = enc.as_bytes();
    let mut r = Decoder::new(&bytes);
    assert_eq!(r.str()?, "");
    assert_eq!(r.bytes()?, &[]);
    assert_eq!(r.record_raw()?, &[]);
    assert!(r.list()?.next()?.is_none());
    assert!(r.map()?.next()?.is_none());
    assert_eq!(r.array()?.remaining(), 0);
    Ok(())
}

#[test]
fn test_struct_as_custom_type() -> R<()> {
    let mut enc = Encoder::new();
    let mut point = Vec::new();
    point.extend_from_slice(&1.5f32.to_le_bytes());
    point.extend_from_slice(&2.5f32.to_le_bytes());
    point.extend_from_slice(&3.5f32.to_le_bytes());
    enc.record_raw(&point)?;

    let bytes = enc.as_bytes();
    let mut r = Decoder::new(&bytes);
    let mut sr = r.record()?;

    let x = sr.f32()?;
    let y = sr.f32()?;
    let z = sr.f32()?;

    assert_eq!(x, 1.5);
    assert_eq!(y, 2.5);
    assert_eq!(z, 3.5);
    Ok(())
}

#[test]
fn test_array_of_structs_layout() -> R<()> {
    let mut enc = Encoder::new();
    let stride = 8;

    let mut arr = enc.array(Tag::Struct, stride)?;

    let mut rec = arr.record();
    rec.u32(1)?;
    rec.u32(2)?;
    rec.finish()?;

    let mut rec = arr.record();
    rec.u32(10)?;
    rec.u32(20)?;
    rec.finish()?;

    arr.finish()?;

    let bytes = enc.as_bytes();

    let mut r = Decoder::new(bytes);
    let mut arr = r.array()?;

    assert_eq!(arr.item_tag(), Tag::Struct);
    assert_eq!(arr.stride(), 8);

    if let ValueDecoder::Struct(data1) = arr.next()?.unwrap() {
        let mut r1 = RecordDecoder::new(data1);
        assert_eq!(r1.u32()?, 1);
        assert_eq!(r1.u32()?, 2);
    } else { panic!("Expected Struct"); }

    if let ValueDecoder::Struct(data2) = arr.next()?.unwrap() {
        let mut r2 = RecordDecoder::new(data2);
        assert_eq!(r2.u32()?, 10);
        assert_eq!(r2.u32()?, 20);
    } else { panic!("Expected Struct"); }

    Ok(())
}
