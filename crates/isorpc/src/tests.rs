use crate::encode_call;
use crate::encode_response_err;
use crate::encode_response_ok;
use isopack::Decoder;
use isopack::ValueDecoder;
use isopack::types::Result;
use wasmtime::component::Val;

type R<T> = Result<T>;

#[test]
fn test_encode_call_basic() -> R<()> {
    let args = vec![
        Val::U32(42),
        Val::String("hello".into()),
    ];
    
    let bytes = encode_call(123, "my_func", &args)?;
    
    let mut dec = Decoder::new(&bytes);
    let mut list = dec.list()?;
    
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
    let value = Val::U64(9999);
    let bytes = encode_response_ok(456, &value)?;
    
    let mut dec = Decoder::new(&bytes);
    let mut list = dec.list()?;
    
    // Check sequence number
    assert_eq!(list.next()?.unwrap().as_u64()?, 456);
    
    // Check Result::Ok
    match list.next()?.unwrap() {
        ValueDecoder::ResultOk => {
            assert_eq!(list.next()?.unwrap().as_u64()?, 9999);
        }
        _ => panic!("Expected ResultOk"),
    }
    
    Ok(())
}

#[test]
fn test_encode_response_err() -> R<()> {
    let bytes = encode_response_err(789, "something went wrong")?;
    
    let mut dec = Decoder::new(&bytes);
    let mut list = dec.list()?;
    
    // Check sequence number
    assert_eq!(list.next()?.unwrap().as_u64()?, 789);
    
    // Check Result::Err
    match list.next()?.unwrap() {
        ValueDecoder::ResultErr => {
            assert_eq!(list.next()?.unwrap().as_str()?, "something went wrong");
        }
        _ => panic!("Expected ResultErr"),
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
    let mut list = dec.list()?;
    
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
    match args_list.next()?.unwrap() {
        ValueDecoder::OptionSome => {
            assert_eq!(args_list.next()?.unwrap().as_str()?, "test");
        }
        _ => panic!("Expected OptionSome"),
    }
    
    Ok(())
}
