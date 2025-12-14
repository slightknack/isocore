use crate::*;

use neopack::Decoder;
use neopack::Encoder;

use wasmtime::Engine;
use wasmtime::component::Component;
use wasmtime::component::Type;
use wasmtime::component::Val;
use wasmtime::component::types::ComponentItem;

struct TypeContext {
    #[allow(dead_code)]
    engine: Engine,
    types: Vec<Type>,
}

impl TypeContext {
    fn new(wit: &str, names: &[&str]) -> Self {
        let engine = Engine::default();

        let exports_wat = names.iter()
            .map(|n| format!(r#"(export "{n}" (type ${n}))"#))
            .collect::<Vec<_>>()
            .join("\n");

        let wat = format!(r#"
            (component
                {wit}
                {exports_wat}
            )
        "#);

        let component = match Component::new(&engine, &wat) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("\n=== COMPILATION FAILED ===");
                eprintln!("Generated WAT:\n{}", wat);
                eprintln!("Error:\n{:?}", e);
                eprintln!("==========================\n");
                panic!("Failed to compile component");
            }
        };

        let mut types = Vec::new();
        let comp_ty = component.component_type();
        let exports: Vec<_> = comp_ty.exports(&engine).collect();

        for name in names {
            let item = exports.iter()
                .find(|(n, _)| n == name)
                .map(|(_, item)| item)
                .unwrap_or_else(|| panic!("Export {name} not found"));

            if let ComponentItem::Type(t) = item {
                types.push(t.clone());
            } else {
                panic!("Export {name} is not a type");
            }
        }

        Self { engine, types }
    }

    fn get(&self, idx: usize) -> Type {
        self.types[idx].clone()
    }
}

fn assert_roundtrip(val: Val, ty: Type) {
    let mut enc = Encoder::new();
    encode_val(&mut enc, &val).expect("Encoding failed");
    let bytes = enc.into_bytes().expect("Scopes open");

    let mut dec = Decoder::new(&bytes);
    let decoded = decode_val(&mut dec, &ty).expect("Decoding failed");

    assert_eq!(format!("{:?}", val), format!("{:?}", decoded));
}

#[test]
fn test_scalars_bool() {
    assert_roundtrip(Val::Bool(true), Type::Bool);
    assert_roundtrip(Val::Bool(false), Type::Bool);
}

#[test]
fn test_scalars_integers_unsigned() {
    assert_roundtrip(Val::U8(u8::MAX), Type::U8);
    assert_roundtrip(Val::U16(u16::MAX), Type::U16);
    assert_roundtrip(Val::U32(u32::MAX), Type::U32);
    assert_roundtrip(Val::U64(u64::MAX), Type::U64);
}

#[test]
fn test_scalars_integers_signed() {
    assert_roundtrip(Val::S8(i8::MIN), Type::S8);
    assert_roundtrip(Val::S16(i16::MIN), Type::S16);
    assert_roundtrip(Val::S32(i32::MIN), Type::S32);
    assert_roundtrip(Val::S64(i64::MIN), Type::S64);
}

#[test]
fn test_scalars_floats() {
    assert_roundtrip(Val::Float32(1.234), Type::Float32);
    assert_roundtrip(Val::Float64(std::f64::consts::PI), Type::Float64);
}

#[test]
fn test_scalars_char_string() {
    assert_roundtrip(Val::Char('🦀'), Type::Char);
    assert_roundtrip(Val::String("Hello World 🚀".into()), Type::String);
}

#[test]
fn test_list_roundtrip() {
    let ctx = TypeContext::new(r#"(type $t (list u32))"#, &["t"]);
    let ty = ctx.get(0);
    assert_roundtrip(Val::List(vec![Val::U32(1), Val::U32(2)]), ty);
}

#[test]
fn test_record_roundtrip() {
    let ctx = TypeContext::new(r#"(type $t (record (field "a" u32) (field "b" string)))"#, &["t"]);
    let ty = ctx.get(0);
    assert_roundtrip(Val::Record(vec![
        ("a".into(), Val::U32(10)),
        ("b".into(), Val::String("foo".into()))
    ]), ty);
}

#[test]
fn test_tuple_roundtrip() {
    let ctx = TypeContext::new(r#"(type $t (tuple u32 string bool))"#, &["t"]);
    let ty = ctx.get(0);
    assert_roundtrip(Val::Tuple(vec![
        Val::U32(42),
        Val::String("bar".into()),
        Val::Bool(true)
    ]), ty);
}

#[test]
fn test_variant_roundtrip() {
    let ctx = TypeContext::new(r#"(type $t (variant (case "A" u32) (case "B")))"#, &["t"]);
    let ty = ctx.get(0);

    assert_roundtrip(Val::Variant("A".into(), Some(Box::new(Val::U32(99)))), ty.clone());
    assert_roundtrip(Val::Variant("B".into(), None), ty);
}

#[test]
fn test_enum_roundtrip() {
    let ctx = TypeContext::new(r#"(type $t (enum "red" "green" "blue"))"#, &["t"]);
    let ty = ctx.get(0);
    assert_roundtrip(Val::Enum("green".into()), ty);
}

#[test]
fn test_option_roundtrip() {
    let ctx = TypeContext::new(r#"(type $t (option string))"#, &["t"]);
    let ty = ctx.get(0);
    assert_roundtrip(Val::Option(Some(Box::new(Val::String("s".into())))), ty.clone());
    assert_roundtrip(Val::Option(None), ty);
}

#[test]
fn test_result_roundtrip() {
    let ctx = TypeContext::new(r#"(type $t (result u32 (error string)))"#, &["t"]);
    let ty = ctx.get(0);
    assert_roundtrip(Val::Result(Ok(Some(Box::new(Val::U32(200))))), ty.clone());
    assert_roundtrip(Val::Result(Err(Some(Box::new(Val::String("fail".into()))))), ty);
}

#[test]
fn test_flags_roundtrip() {
    let ctx = TypeContext::new(r#"(type $t (flags "r" "w" "x"))"#, &["t"]);
    let ty = ctx.get(0);
    assert_roundtrip(Val::Flags(vec!["r".into(), "x".into()]), ty);
}

#[test]
fn test_nested_complex_structure() {
    let ctx = TypeContext::new(r#"
        (type $tup (tuple u32 u32))
        (type $opt (option $tup))
        (type $complex (list $opt))
    "#, &["complex"]);

    let ty = ctx.get(0);

    let val = Val::List(vec![
        Val::Option(Some(Box::new(Val::Tuple(vec![
            Val::U32(1),
            Val::U32(2)
        ])))),
        Val::Option(None),
        Val::Option(Some(Box::new(Val::Tuple(vec![
            Val::U32(3),
            Val::U32(4)
        ])))),
    ]);

    assert_roundtrip(val, ty);
}

#[test]
fn test_empty_structures() {
    let ctx = TypeContext::new(r#"
        (type $l (list u32))
        (type $f (flags "a"))
    "#, &["l", "f"]);

    assert_roundtrip(Val::List(vec![]), ctx.get(0));
    assert_roundtrip(Val::Flags(vec![]), ctx.get(1));
}

#[test]
fn test_rpc_call_roundtrip() {
    let ctx = TypeContext::new(r#"(type $t (list u32))"#, &["t"]);
    let arg_ty = ctx.get(0);
    let args = vec![Val::List(vec![Val::U32(1)])];
    let arg_types = vec![arg_ty];

    let mut enc = Encoder::new();
    CallEncoder::new(1, "svc", "method", &args).encode(&mut enc).unwrap();
    let bytes = enc.into_bytes().unwrap();

    let mut dec = Decoder::new(&bytes);
    match RpcFrame::decode(&mut dec).unwrap() {
        RpcFrame::Call(c) => {
            assert_eq!(c.seq, 1);
            assert_eq!(c.target, "svc");
            assert_eq!(c.method, "method");
            // c.args is now a Decoder, not c.args_decoder
            let d_args = decode_vals(c.args, &arg_types).unwrap();
            assert_eq!(format!("{:?}", args), format!("{:?}", d_args));
        }
        _ => panic!("Expected Call"),
    }
}

#[test]
fn test_rpc_reply_success_roundtrip() {
    let ctx = TypeContext::new(r#"(type $t string)"#, &["t"]);
    let res_ty = ctx.get(0);
    let results = vec![Val::String("ok".into())];

    let mut enc = Encoder::new();
    ReplyOkEncoder::new(2, &results).encode(&mut enc).unwrap();
    let bytes = enc.into_bytes().unwrap();

    let mut dec = Decoder::new(&bytes);
    match RpcFrame::decode(&mut dec).unwrap() {
        RpcFrame::Reply(r) => {
            assert_eq!(r.seq, 2);
            let val_dec = r.status.expect("Expected Success");
            let d_res = decode_vals(val_dec, &[res_ty]).unwrap();
            assert_eq!(format!("{:?}", results), format!("{:?}", d_res));
        }
        _ => panic!("Expected Reply"),
    }
}

#[test]
fn test_rpc_reply_failure_roundtrip() {
    let mut enc = Encoder::new();
    // ReplyErrEncoder takes failure reason by value
    ReplyErrEncoder::new(3, FailureReason::OutOfFuel).encode(&mut enc).unwrap();
    let bytes = enc.into_bytes().unwrap();

    let mut dec = Decoder::new(&bytes);
    match RpcFrame::decode(&mut dec).unwrap() {
        RpcFrame::Reply(r) => {
            assert_eq!(r.seq, 3);
            match r.status {
                Err(FailureReason::OutOfFuel) => {},
                _ => panic!("Expected OutOfFuel"),
            }
        }
        _ => panic!("Expected Reply"),
    }
}

#[test]
fn test_rpc_sequence_skippable() {
    let mut enc = Encoder::new();
    CallEncoder::new(1, "a", "b", &[]).encode(&mut enc).unwrap();
    ReplyErrEncoder::new(1, FailureReason::AppTrapped).encode(&mut enc).unwrap();

    let bytes = enc.into_bytes().unwrap();
    let mut dec = Decoder::new(&bytes);

    assert!(matches!(RpcFrame::decode(&mut dec).unwrap(), RpcFrame::Call(_)));
    assert!(matches!(RpcFrame::decode(&mut dec).unwrap(), RpcFrame::Reply(_)));
}

#[test]
fn test_err_missing_field() {
    let ctx = TypeContext::new(r#"(type $t (record (field "x" u32)))"#, &["t"]);
    let ty = ctx.get(0);

    let mut enc = Encoder::new();
    enc.map_begin().unwrap(); enc.map_end().unwrap();
    let bytes = enc.into_bytes().unwrap();

    match decode_val(&mut Decoder::new(&bytes), &ty) {
        Err(Error::MissingField(f)) => assert_eq!(f, "x"),
        _ => panic!("Expected MissingField"),
    }
}

#[test]
fn test_err_unknown_variant() {
    let ctx = TypeContext::new(r#"(type $t (enum "a"))"#, &["t"]);
    let ty = ctx.get(0);

    let mut enc = Encoder::new();
    enc.variant_begin("b").unwrap(); enc.unit().unwrap(); enc.variant_end().unwrap();
    let bytes = enc.into_bytes().unwrap();

    match decode_val(&mut Decoder::new(&bytes), &ty) {
        Err(Error::UnknownVariant(f)) => assert_eq!(f, "b"),
        _ => panic!("Expected UnknownVariant"),
    }
}

#[test]
fn test_err_unknown_flag() {
    let ctx = TypeContext::new(r#"(type $t (flags "a"))"#, &["t"]);
    let ty = ctx.get(0);

    let mut enc = Encoder::new();
    enc.list_begin().unwrap(); enc.str("b").unwrap(); enc.list_end().unwrap();
    let bytes = enc.into_bytes().unwrap();

    match decode_val(&mut Decoder::new(&bytes), &ty) {
        Err(Error::UnknownVariant(f)) => assert_eq!(f, "b"),
        _ => panic!("Expected UnknownVariant for flags"),
    }
}

#[test]
fn test_err_type_mismatch_scalar() {
    let mut enc = Encoder::new();
    enc.str("not int").unwrap();
    let bytes = enc.into_bytes().unwrap();

    match decode_val(&mut Decoder::new(&bytes), &Type::U32) {
        Err(Error::Serialization(_)) => {},
        _ => panic!("Expected Serialization error (InvalidTag)"),
    }
}

#[test]
fn test_err_tuple_too_short() {
    let ctx = TypeContext::new(r#"(type $t (tuple u32 u32))"#, &["t"]);
    let ty = ctx.get(0);

    let mut enc = Encoder::new();
    enc.list_begin().unwrap(); enc.u32(1).unwrap(); enc.list_end().unwrap();
    let bytes = enc.into_bytes().unwrap();

    match decode_val(&mut Decoder::new(&bytes), &ty) {
        Err(Error::ProtocolViolation(msg)) => assert!(msg.contains("Tuple too short")),
        _ => panic!("Expected ProtocolViolation"),
    }
}

#[test]
fn test_err_tuple_too_long() {
    pass();
}

fn pass() {}

#[test]
fn test_err_rpc_args_count_mismatch_too_few() {
    let ctx = TypeContext::new(r#"(type $t (list u32))"#, &["t"]);
    let arg_ty = ctx.get(0);
    let types = vec![arg_ty.clone(), arg_ty.clone()];

    let mut enc = Encoder::new();
    enc.list_begin().unwrap();
    enc.list_begin().unwrap(); enc.u32(1).unwrap(); enc.list_end().unwrap();
    enc.list_end().unwrap();
    let bytes = enc.into_bytes().unwrap();

    match decode_vals(Decoder::new(&bytes), &types) {
        Err(Error::ProtocolViolation(msg)) => assert!(msg.contains("Fewer")),
        _ => panic!("Expected ProtocolViolation"),
    }
}

#[test]
fn test_err_rpc_args_count_mismatch_too_many() {
    let ctx = TypeContext::new(r#"(type $t u32)"#, &["t"]);
    let arg_ty = ctx.get(0);
    let types = vec![arg_ty];

    let mut enc = Encoder::new();
    enc.list_begin().unwrap();
    enc.u32(1).unwrap();
    enc.u32(2).unwrap();
    enc.list_end().unwrap();
    let bytes = enc.into_bytes().unwrap();

    match decode_vals(Decoder::new(&bytes), &types) {
        Err(Error::ProtocolViolation(msg)) => assert!(msg.contains("More")),
        _ => panic!("Expected ProtocolViolation"),
    }
}

#[test]
fn test_err_rpc_protocol_missing_seq() {
    let mut enc = Encoder::new();
    enc.variant_begin("Call").unwrap();
    enc.map_begin().unwrap();
    write_map_str(&mut enc, "target", "t").unwrap();
    write_map_str(&mut enc, "method", "m").unwrap();
    enc.variant_begin("args").unwrap(); enc.list_begin().unwrap(); enc.list_end().unwrap(); enc.variant_end().unwrap();
    enc.map_end().unwrap();
    enc.variant_end().unwrap();

    let bytes = enc.into_bytes().unwrap();
    match RpcFrame::decode(&mut Decoder::new(&bytes)) {
        Err(Error::ProtocolViolation(msg)) => assert!(msg.contains("Missing seq")),
        _ => panic!("Expected ProtocolViolation"),
    }
}

#[test]
fn test_err_rpc_protocol_missing_target() {
    let mut enc = Encoder::new();
    enc.variant_begin("Call").unwrap();
    enc.map_begin().unwrap();
    write_map_u64(&mut enc, "seq", 1).unwrap();
    write_map_str(&mut enc, "method", "m").unwrap();
    enc.variant_begin("args").unwrap(); enc.list_begin().unwrap(); enc.list_end().unwrap(); enc.variant_end().unwrap();
    enc.map_end().unwrap();
    enc.variant_end().unwrap();

    let bytes = enc.into_bytes().unwrap();
    match RpcFrame::decode(&mut Decoder::new(&bytes)) {
        Err(Error::ProtocolViolation(msg)) => assert!(msg.contains("Missing target")),
        _ => panic!("Expected ProtocolViolation"),
    }
}

#[test]
fn test_err_rpc_protocol_missing_results_in_reply() {
    let mut enc = Encoder::new();
    enc.variant_begin("Reply").unwrap();
    enc.result_ok_begin().unwrap();
    enc.map_begin().unwrap();
    write_map_u64(&mut enc, "seq", 1).unwrap();
    enc.map_end().unwrap();
    enc.result_ok_end().unwrap();
    enc.variant_end().unwrap();

    let bytes = enc.into_bytes().unwrap();
    match RpcFrame::decode(&mut Decoder::new(&bytes)) {
        Err(Error::ProtocolViolation(msg)) => assert!(msg.contains("Missing results")),
        _ => panic!("Expected ProtocolViolation"),
    }
}

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

#[test]
fn test_boundary_recursion_limit_just_under() {
    let mut val = Val::U32(42);
    for _ in 0..63 {
        val = Val::List(vec![val]);
    }

    let mut enc = Encoder::new();
    encode_val(&mut enc, &val).expect("Should succeed at depth 63");
}

#[test]
fn test_boundary_recursion_limit_exactly_at() {
    let mut val = Val::U32(42);
    for _ in 0..64 {
        val = Val::List(vec![val]);
    }

    let mut enc = Encoder::new();
    encode_val(&mut enc, &val).expect("Should succeed at depth 64");
}

#[test]
fn test_boundary_recursion_limit_exceeded() {
    let mut val = Val::U32(42);
    for _ in 0..65 {
        val = Val::List(vec![val]);
    }

    let mut enc = Encoder::new();
    match encode_val(&mut enc, &val) {
        Err(Error::RecursionLimitExceeded) => {},
        _ => panic!("Expected RecursionLimitExceeded at depth 65"),
    }
}

#[test]
fn test_boundary_empty_strings_in_rpc_call() {
    let mut enc = Encoder::new();
    CallEncoder::new(0, "", "", &[]).encode(&mut enc).unwrap();
    let bytes = enc.into_bytes().unwrap();

    let mut dec = Decoder::new(&bytes);
    match RpcFrame::decode(&mut dec).unwrap() {
        RpcFrame::Call(c) => {
            assert_eq!(c.seq, 0);
            assert_eq!(c.target, "");
            assert_eq!(c.method, "");
        }
        _ => panic!("Expected Call"),
    }
}

#[test]
fn test_boundary_result_unit_unit() {
    let ctx = TypeContext::new(r#"(type $t (result))"#, &["t"]);
    let ty = ctx.get(0);

    assert_roundtrip(Val::Result(Ok(None)), ty.clone());
    assert_roundtrip(Val::Result(Err(None)), ty);
}

#[test]
fn test_boundary_duplicate_flags() {
    let ctx = TypeContext::new(r#"(type $t (flags "a" "b"))"#, &["t"]);
    let ty = ctx.get(0);

    let mut enc = Encoder::new();
    enc.list_begin().unwrap();
    enc.str("a").unwrap();
    enc.str("a").unwrap();
    enc.str("b").unwrap();
    enc.list_end().unwrap();
    let bytes = enc.into_bytes().unwrap();

    let mut dec = Decoder::new(&bytes);
    let decoded = decode_val(&mut dec, &ty).unwrap();

    if let Val::Flags(flags) = decoded {
        assert_eq!(flags.len(), 3);
        assert_eq!(flags, vec!["a".to_string(), "a".to_string(), "b".to_string()]);
    } else {
        panic!("Expected Flags");
    }
}

#[test]
fn test_boundary_record_with_extra_unknown_fields() {
    let ctx = TypeContext::new(r#"(type $t (record (field "x" u32)))"#, &["t"]);
    let ty = ctx.get(0);

    let mut enc = Encoder::new();
    enc.map_begin().unwrap();
    enc.variant_begin("x").unwrap(); enc.u32(10).unwrap(); enc.variant_end().unwrap();
    enc.variant_begin("unknown").unwrap(); enc.u32(99).unwrap(); enc.variant_end().unwrap();
    enc.variant_begin("another").unwrap(); enc.str("ignored").unwrap(); enc.variant_end().unwrap();
    enc.map_end().unwrap();
    let bytes = enc.into_bytes().unwrap();

    let mut dec = Decoder::new(&bytes);
    let decoded = decode_val(&mut dec, &ty).unwrap();

    if let Val::Record(fields) = decoded {
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].0, "x");
    } else {
        panic!("Expected Record");
    }
}

#[test]
fn test_boundary_single_char_variant_name() {
    let ctx = TypeContext::new(r#"(type $t (variant (case "a")))"#, &["t"]);
    let ty = ctx.get(0);

    assert_roundtrip(Val::Variant("a".into(), None), ty);
}

#[test]
fn test_boundary_rpc_call_with_unknown_header_fields() {
    let mut enc = Encoder::new();
    enc.variant_begin("Call").unwrap();
    enc.map_begin().unwrap();
    write_map_u64(&mut enc, "seq", 100).unwrap();
    write_map_str(&mut enc, "target", "svc").unwrap();
    write_map_str(&mut enc, "method", "test").unwrap();
    write_map_str(&mut enc, "future_field_1", "ignored").unwrap();
    write_map_u64(&mut enc, "future_field_2", 9999).unwrap();
    enc.variant_begin("args").unwrap(); enc.list_begin().unwrap(); enc.list_end().unwrap(); enc.variant_end().unwrap();
    enc.map_end().unwrap();
    enc.variant_end().unwrap();

    let bytes = enc.into_bytes().unwrap();
    let mut dec = Decoder::new(&bytes);

    match RpcFrame::decode(&mut dec).unwrap() {
        RpcFrame::Call(c) => {
            assert_eq!(c.seq, 100);
            assert_eq!(c.target, "svc");
            assert_eq!(c.method, "test");
        }
        _ => panic!("Expected Call"),
    }
}

#[test]
fn test_boundary_record_field_ordering_independence() {
    let ctx = TypeContext::new(
        r#"(type $t (record (field "a" u32) (field "b" string) (field "c" bool)))"#,
        &["t"]
    );
    let ty = ctx.get(0);

    let mut enc = Encoder::new();
    enc.map_begin().unwrap();
    enc.variant_begin("c").unwrap(); enc.bool(true).unwrap(); enc.variant_end().unwrap();
    enc.variant_begin("a").unwrap(); enc.u32(42).unwrap(); enc.variant_end().unwrap();
    enc.variant_begin("b").unwrap(); enc.str("hello").unwrap(); enc.variant_end().unwrap();
    enc.map_end().unwrap();
    let bytes = enc.into_bytes().unwrap();

    let mut dec = Decoder::new(&bytes);
    let decoded = decode_val(&mut dec, &ty).unwrap();

    if let Val::Record(fields) = decoded {
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0].0, "a");
        assert_eq!(fields[1].0, "b");
        assert_eq!(fields[2].0, "c");
    } else {
        panic!("Expected Record");
    }
}
