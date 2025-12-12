// crates/isorpc/src/tests.rs
use isopack::Decoder;
use isopack::ValueDecoder;
use wasmtime::component::Val;

use crate::types::Result;
use crate::message::encode_call;
use crate::message::encode_response_ok;
use crate::message::encode_response_err;

type R<T> = Result<T>;

#[test]
fn test_encode_call_basic() -> R<()> {
    let args = vec![
        Val::U32(42),
        Val::String("hello".into()),
    ];

    let bytes = encode_call(123, "my_func", &args)?;

    let mut dec = Decoder::new(&bytes);

    // Outer variant "call"
    let name = dec.value()?.as_variant()?;
    let payload = dec.value()?;
    assert_eq!(name, "call");

    let mut list = match payload {
        ValueDecoder::List(l) => l,
        _ => panic!("Expected list payload"),
    };

    // Check sequence number
    assert_eq!(list.next()?.unwrap().as_u64()?, 123);

    // Check function name
    assert_eq!(list.next()?.unwrap().as_str()?, "my_func");

    // Check args list
    let mut args_list = match list.next()?.unwrap() {
        ValueDecoder::List(l) => l,
        _ => panic!("Expected list"),
    };

    assert_eq!(args_list.next()?.unwrap().as_u32()?, 42);
    assert_eq!(args_list.next()?.unwrap().as_str()?, "hello");

    Ok(())
}

#[test]
fn test_encode_response_ok() -> R<()> {
    let values = vec![Val::U64(9999)];
    let bytes = encode_response_ok(456, &values)?;

    let mut dec = Decoder::new(&bytes);

    // Outer variant "resp"
    let (name, payload) = dec.value()?.as_variant()?;
    assert_eq!(name, "resp");

    let mut list = match payload {
        ValueDecoder::List(l) => l,
        _ => panic!("Expected list payload"),
    };

    // Check sequence number
    assert_eq!(list.next()?.unwrap().as_u64()?, 456);

    // Check Result::Ok
    let result_val = list.next()?.unwrap();
    match result_val.as_result()? {
        Ok(inner_box) => {
            let mut val_list = match **inner_box {
                ValueDecoder::List(ref l) => l.clone(), // clone to iterate
                _ => panic!("Expected list of return values"),
            };
            assert_eq!(val_list.next()?.unwrap().as_u64()?, 9999);
        }
        Err(_) => panic!("Expected ResultOk"),
    }

    Ok(())
}

#[test]
fn test_encode_response_err() -> R<()> {
    let bytes = encode_response_err(789, "something went wrong")?;

    let mut dec = Decoder::new(&bytes);

    // Outer variant "resp"
    let (name, payload) = dec.value()?.as_variant()?;
    assert_eq!(name, "resp");

    let mut list = match payload {
        ValueDecoder::List(l) => l,
        _ => panic!("Expected list payload"),
    };

    // Check sequence number
    assert_eq!(list.next()?.unwrap().as_u64()?, 789);

    // Check Result::Err
    let result_val = list.next()?.unwrap();
    match result_val.as_result()? {
        Ok(_) => panic!("Expected ResultErr"),
        Err(e) => assert_eq!(e.as_str()?, "something went wrong"),
    }

    Ok(())
}

#[test]
fn test_call_with_complex_args() -> R<()> {
    let args = vec![
        Val::List(vec![Val::U32(1), Val::U32(2), Val::U32(3)]),
        Val::Option(Some(Box::new(Val::String("test".into())))),
    ];

    let bytes = encode_call(1, "complex_func", &args)?;

    let mut dec = Decoder::new(&bytes);
    let (_, payload) = dec.value()?.as_variant()?;
    let mut list = match payload {
        ValueDecoder::List(l) => l,
        _ => panic!("Expected list"),
    };

    list.next()?; // skip seq
    list.next()?; // skip func name

    let mut args_list = match list.next()?.unwrap() {
        ValueDecoder::List(l) => l,
        _ => panic!("Expected list"),
    };

    // First arg: list [1,2,3]
    let mut inner_list = match args_list.next()?.unwrap() {
        ValueDecoder::List(l) => l,
        _ => panic!("Expected list"),
    };
    assert_eq!(inner_list.next()?.unwrap().as_u32()?, 1);
    assert_eq!(inner_list.next()?.unwrap().as_u32()?, 2);
    assert_eq!(inner_list.next()?.unwrap().as_u32()?, 3);

    // Second arg: Option::Some("test")
    let opt_val = args_list.next()?.unwrap();
    let opt = opt_val.as_option()?;
    assert_eq!(opt.unwrap().as_str()?, "test");

    Ok(())
}
