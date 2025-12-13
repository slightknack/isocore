//! Tests for the Proxy with mock transports.

use std::sync::Arc;

use neopack::Decoder;
use neopack::Encoder;
use neorpc::RpcFrame;
use neorpc::decode_vals;
use neorpc::CallEncoder;
use neorpc::FailureReason;
use neorpc::ReplyOkEncoder;
use neorpc::ReplyErrEncoder;
use wasmtime::component::Val;
use wasmtime::component::Type;

use crate::proxy::Proxy;
use crate::proxy::ProxyError;
use crate::ledger::FunctionSignature;
use crate::transport;
use crate::transport::Transport;
use crate::transport::TransportError;

/// Mock transport that implements a ping->pong server.
/// Expects a single string argument "ping" and returns "pong".
struct PingPongTransport;

impl PingPongTransport {
    fn decode_frame<'a>(&self, payload: &'a [u8]) -> transport::Result<RpcFrame<'a>> {
        let mut dec = Decoder::new(payload);
        RpcFrame::decode(&mut dec).map_err(|e| {
            TransportError::Io(format!("Failed to decode: {}", e))
        })
    }

    fn decode_args(&self, args: Decoder) -> transport::Result<Vec<Val>> {
        decode_vals(args, &[Type::String]).map_err(|e| {
            TransportError::Io(format!("Failed to decode arg: {}", e))
        })
    }

    fn validate_ping(&self, args: &[Val]) -> transport::Result<()> {
        if args.len() != 1 {
            return Err(TransportError::Io("Expected exactly one argument".into()));
        }
        match &args[0] {
            Val::String(s) if s == "ping" => Ok(()),
            _ => Err(TransportError::Io("Expected 'ping' string argument".into()))
        }
    }

    fn encode_pong(&self, seq: u64) -> transport::Result<Vec<u8>> {
        let mut enc = Encoder::new();
        ReplyOkEncoder::new(seq, &[Val::String("pong".into())]).encode(&mut enc).map_err(|e| {
            TransportError::Io(format!("Failed to encode reply: {}", e))
        })?;
        enc.into_bytes().map_err(|e| {
            TransportError::Io(format!("Failed to finalize bytes: {}", e))
        })
    }
}

#[async_trait::async_trait]
impl Transport for PingPongTransport {
    async fn call(&self, payload: &[u8]) -> transport::Result<Vec<u8>> {
        let frame = self.decode_frame(payload)?;

        match frame {
            RpcFrame::Call(call) => {
                let args = self.decode_args(call.args)?;
                self.validate_ping(&args)?;
                self.encode_pong(call.seq)
            }
            RpcFrame::Reply(_) => {
                Err(TransportError::Io("Received Reply frame in transport".into()))
            }
        }
    }
}

/// Mock transport that returns a Call frame instead of Reply (protocol violation).
/// Note: Hardcoded `seq = 999`.
struct CallReturningTransport;

#[async_trait::async_trait]
impl Transport for CallReturningTransport {
    async fn call(&self, _payload: &[u8]) -> transport::Result<Vec<u8>> {
        let mut enc = Encoder::new();
        CallEncoder::new(999, "target", "method", &[]).encode(&mut enc).unwrap();
        Ok(enc.into_bytes().unwrap())
    }
}

/// Mock transport that returns a reply with wrong sequence number.
/// Note: Hardcoded `seq = 999`; tests must use a different sequence number.
struct WrongSeqTransport;

#[async_trait::async_trait]
impl Transport for WrongSeqTransport {
    async fn call(&self, _payload: &[u8]) -> transport::Result<Vec<u8>> {
        let mut enc = Encoder::new();
        ReplyOkEncoder::new(999, &[]).encode(&mut enc).unwrap();
        Ok(enc.into_bytes().unwrap())
    }
}

/// Mock transport that returns a failure reply.
struct FailureTransport;

#[async_trait::async_trait]
impl Transport for FailureTransport {
    async fn call(&self, payload: &[u8]) -> crate::transport::Result<Vec<u8>> {
        // Decode to get seq
        let mut dec = Decoder::new(payload);
        let frame = RpcFrame::decode(&mut dec).unwrap();
        let seq = match frame {
            RpcFrame::Call(call) => call.seq,
            _ => 0,
        };

        let mut enc = Encoder::new();
        ReplyErrEncoder::new(seq, FailureReason::AppTrapped).encode(&mut enc).unwrap();
        Ok(enc.into_bytes().unwrap())
    }
}

/// Mock transport that always times out.
struct TimeoutTransport;

#[async_trait::async_trait]
impl Transport for TimeoutTransport {
    async fn call(&self, _payload: &[u8]) -> crate::transport::Result<Vec<u8>> {
        Err(TransportError::Timeout)
    }
}

/// Mock transport that returns malformed bytes.
struct MalformedTransport;

#[async_trait::async_trait]
impl Transport for MalformedTransport {
    async fn call(&self, _payload: &[u8]) -> crate::transport::Result<Vec<u8>> {
        Ok(vec![0xFF, 0xFF, 0xFF])
    }
}

fn make_signature(params: Vec<wasmtime::component::Type>, results: Vec<wasmtime::component::Type>) -> FunctionSignature {
    FunctionSignature { params, results }
}

#[tokio::test]
async fn test_successful_ping_pong() {
    let transport = Arc::new(PingPongTransport) as Arc<dyn Transport>;

    // Create a signature expecting string -> string
    let string_ty = wasmtime::component::Type::String;
    let sig = make_signature(vec![string_ty.clone()], vec![string_ty]);

    let args = vec![Val::String("ping".into())];
    let call = CallEncoder::new(1, "target", "method", &args);

    let results = Proxy::invoke(call, &transport, &sig).await.unwrap();

    assert_eq!(results.len(), 1);
    match &results[0] {
        Val::String(s) => assert_eq!(s, "pong"),
        _ => panic!("Expected String"),
    }
}

#[tokio::test]
async fn test_transport_error() {
    let transport = Arc::new(TimeoutTransport) as Arc<dyn Transport>;

    let u32_ty = wasmtime::component::Type::U32;
    let sig = make_signature(vec![u32_ty.clone()], vec![u32_ty]);

    let args = vec![Val::U32(42)];
    let call = CallEncoder::new(1, "target", "method", &args);

    let err = Proxy::invoke(call, &transport, &sig).await.unwrap_err();

    match err {
        ProxyError::Transport(TransportError::Timeout) => {},
        _ => panic!("Expected Transport(Timeout), got {:?}", err),
    }
}

#[tokio::test]
async fn test_rpc_protocol_violation_call_frame() {
    let transport = Arc::new(CallReturningTransport) as Arc<dyn Transport>;

    let u32_ty = wasmtime::component::Type::U32;
    let sig = make_signature(vec![u32_ty.clone()], vec![u32_ty]);

    let args = vec![Val::U32(42)];
    let call = CallEncoder::new(1, "target", "method", &args);

    let err = Proxy::invoke(call, &transport, &sig).await.unwrap_err();

    match err {
        ProxyError::Rpc(e) => {
            assert!(format!("{}", e).contains("Received Call frame"));
        },
        _ => panic!("Expected Rpc error, got {:?}", err),
    }
}

#[tokio::test]
async fn test_rpc_malformed_frame() {
    let transport = Arc::new(MalformedTransport) as Arc<dyn Transport>;

    let u32_ty = wasmtime::component::Type::U32;
    let sig = make_signature(vec![u32_ty.clone()], vec![u32_ty]);

    let args = vec![Val::U32(42)];
    let call = CallEncoder::new(1, "target", "method", &args);

    let err = Proxy::invoke(call, &transport, &sig).await.unwrap_err();

    match err {
        ProxyError::Rpc(_) => {},
        _ => panic!("Expected Rpc error, got {:?}", err),
    }
}

#[tokio::test]
async fn test_remote_failure() {
    let transport = Arc::new(FailureTransport) as Arc<dyn Transport>;

    let u32_ty = wasmtime::component::Type::U32;
    let sig = make_signature(vec![u32_ty.clone()], vec![u32_ty]);

    let args = vec![Val::U32(42)];
    let call = CallEncoder::new(1, "target", "method", &args);

    let err = Proxy::invoke(call, &transport, &sig).await.unwrap_err();

    match err {
        ProxyError::Remote(FailureReason::AppTrapped) => {},
        _ => panic!("Expected Remote(AppTrapped), got {:?}", err),
    }
}

#[tokio::test]
async fn test_internal_sequence_mismatch() {
    let transport = Arc::new(WrongSeqTransport) as Arc<dyn Transport>;

    let u32_ty = wasmtime::component::Type::U32;
    let sig = make_signature(vec![u32_ty.clone()], vec![u32_ty]);

    let args = vec![Val::U32(42)];
    let call = CallEncoder::new(1, "target", "method", &args);

    let err = Proxy::invoke(call, &transport, &sig).await.unwrap_err();

    match err {
        ProxyError::Internal(msg) => {
            assert!(msg.contains("Sequence mismatch"));
        },
        _ => panic!("Expected Internal error, got {:?}", err),
    }
}
