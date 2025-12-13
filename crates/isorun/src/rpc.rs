//! Generic RPC system for isorun.
//!
//! Built on neorpc's protocol:
//! - Call: [seq, target, method, [args...]]
//! - Reply: [seq, Result<[results...], FailureReason>]
//!
//! This module is completely application-agnostic. It knows nothing about
//! specific WIT interfaces or functions.

use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

pub use neorpc::CallFrame;
pub use neorpc::FailureReason;
pub use neorpc::ReplyFrame;
pub use neorpc::RpcFrame;

/// Global sequence number generator for RPC calls.
static NEXT_SEQ: AtomicU64 = AtomicU64::new(1);

/// Generate a unique sequence number for an RPC call.
pub fn next_seq() -> u64 {
    NEXT_SEQ.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    use neopack::Decoder;
    use neopack::Encoder;

    use neorpc::decode_vals;

    use wasmtime::component::Type;
    use wasmtime::component::Val;

    #[test]
    fn test_seq_generator_increments() {
        let s1 = next_seq();
        let s2 = next_seq();
        let s3 = next_seq();
        
        assert!(s2 > s1);
        assert!(s3 > s2);
    }

    #[test]
    fn test_call_frame_encode_decode() {
        let seq = next_seq();
        let args = vec![Val::U32(42), Val::String("hello".into())];
        
        let mut enc = Encoder::new();
        CallFrame::encode(&mut enc, seq, "instance-123", "my_method", &args).unwrap();
        let bytes = enc.into_bytes().unwrap();
        
        let mut dec = Decoder::new(&bytes);
        match RpcFrame::decode(&mut dec).unwrap() {
            RpcFrame::Call(call) => {
                assert_eq!(call.seq, seq);
                assert_eq!(call.target, "instance-123");
                assert_eq!(call.method, "my_method");
                
                let arg_types = vec![Type::U32, Type::String];
                let decoded_args = decode_vals(call.args_decoder, &arg_types).unwrap();
                
                assert_eq!(decoded_args.len(), 2);
                assert!(matches!(decoded_args[0], Val::U32(42)));
                if let Val::String(s) = &decoded_args[1] {
                    assert_eq!(s, "hello");
                }
            }
            _ => panic!("Expected Call frame"),
        }
    }

    #[test]
    fn test_reply_frame_success_encode_decode() {
        let seq = next_seq();
        let results = vec![Val::U64(9999)];
        
        let mut enc = Encoder::new();
        ReplyFrame::encode_success(&mut enc, seq, &results).unwrap();
        let bytes = enc.into_bytes().unwrap();
        
        let mut dec = Decoder::new(&bytes);
        match RpcFrame::decode(&mut dec).unwrap() {
            RpcFrame::Reply(reply) => {
                assert_eq!(reply.seq, seq);
                match reply.status {
                    Ok(results_dec) => {
                        let result_types = vec![Type::U64];
                        let decoded_results = decode_vals(results_dec, &result_types).unwrap();
                        
                        assert_eq!(decoded_results.len(), 1);
                        assert!(matches!(decoded_results[0], Val::U64(9999)));
                    }
                    Err(_) => panic!("Expected success"),
                }
            }
            _ => panic!("Expected Reply frame"),
        }
    }

    #[test]
    fn test_reply_frame_failure_encode_decode() {
        let seq = next_seq();
        
        let mut enc = Encoder::new();
        ReplyFrame::encode_failure(&mut enc, seq, &FailureReason::MethodNotFound).unwrap();
        let bytes = enc.into_bytes().unwrap();
        
        let mut dec = Decoder::new(&bytes);
        match RpcFrame::decode(&mut dec).unwrap() {
            RpcFrame::Reply(reply) => {
                assert_eq!(reply.seq, seq);
                match reply.status {
                    Err(reason) => {
                        assert_eq!(reason, FailureReason::MethodNotFound);
                    }
                    Ok(_) => panic!("Expected failure"),
                }
            }
            _ => panic!("Expected Reply frame"),
        }
    }

    #[test]
    fn test_empty_args_and_results() {
        let seq = next_seq();
        
        let mut enc = Encoder::new();
        CallFrame::encode(&mut enc, seq, "target", "method", &[]).unwrap();
        let bytes = enc.into_bytes().unwrap();
        
        let mut dec = Decoder::new(&bytes);
        match RpcFrame::decode(&mut dec).unwrap() {
            RpcFrame::Call(call) => {
                let decoded_args = decode_vals(call.args_decoder, &[]).unwrap();
                assert_eq!(decoded_args.len(), 0);
            }
            _ => panic!("Expected Call frame"),
        }
    }

    #[test]
    fn test_multiple_results() {
        let seq = next_seq();
        let results = vec![
            Val::Bool(true),
            Val::U32(42),
            Val::String("result".into()),
        ];
        
        let mut enc = Encoder::new();
        ReplyFrame::encode_success(&mut enc, seq, &results).unwrap();
        let bytes = enc.into_bytes().unwrap();
        
        let mut dec = Decoder::new(&bytes);
        match RpcFrame::decode(&mut dec).unwrap() {
            RpcFrame::Reply(reply) => {
                match reply.status {
                    Ok(results_dec) => {
                        let result_types = vec![Type::Bool, Type::U32, Type::String];
                        let decoded = decode_vals(results_dec, &result_types).unwrap();
                        
                        assert_eq!(decoded.len(), 3);
                        assert!(matches!(decoded[0], Val::Bool(true)));
                        assert!(matches!(decoded[1], Val::U32(42)));
                    }
                    Err(_) => panic!("Expected success"),
                }
            }
            _ => panic!("Expected Reply frame"),
        }
    }
}
