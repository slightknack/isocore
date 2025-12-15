//! Tests for RPC invocation with mock transports.

use std::sync::Arc;

use neopack::Decoder;
use neopack::Encoder;
use neorpc::CallEncoder;
use neorpc::FailureReason;
use neorpc::ReplyOkEncoder;
use neorpc::ReplyErrEncoder;
use neorpc::RpcFrame;
use neorpc::decode_vals;
use neorpc::encode_vals_to_bytes;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use wasmtime::component::Type;
use wasmtime::component::Val;

use crate::peer;
use crate::peer::Peer;
use crate::transport;
use crate::transport::Transport;

/// A duplex channel transport using tokio mpsc channels.
///
/// This mock allows simulating bidirectional communication for testing.
/// Messages sent via send() appear on the peer's recv() and vice versa.
struct DuplexChannelTransport {
    tx: mpsc::UnboundedSender<Vec<u8>>,
    rx: Arc<Mutex<mpsc::UnboundedReceiver<Vec<u8>>>>,
}

impl DuplexChannelTransport {
    fn new(
        tx: mpsc::UnboundedSender<Vec<u8>>,
        rx: mpsc::UnboundedReceiver<Vec<u8>>,
    ) -> Self {
        Self {
            tx,
            rx: Arc::new(Mutex::new(rx)),
        }
    }
}

#[async_trait::async_trait]
impl Transport for DuplexChannelTransport {
    async fn send(&self, payload: &[u8]) -> transport::Result<()> {
        self.tx
            .send(payload.to_vec())
            .map_err(|_| transport::Error::ConnectionLost("Channel closed".into()))
    }

    async fn recv(&self) -> transport::Result<Option<Vec<u8>>> {
        let mut rx = self.rx.lock().await;
        Ok(rx.recv().await)
    }
}

/// Mock transport that implements a ping->pong server.
/// Expects a single string argument "ping" and returns "pong".
struct PingPongTransport {
    pending: Arc<Mutex<Option<Vec<u8>>>>,
}

impl PingPongTransport {
    fn new() -> Self {
        Self {
            pending: Arc::new(Mutex::new(None)),
        }
    }

    fn decode_frame<'a>(&self, payload: &'a [u8]) -> transport::Result<RpcFrame<'a>> {
        let mut dec = Decoder::new(payload);
        RpcFrame::decode(&mut dec).map_err(|e| {
            transport::Error::Io(format!("Failed to decode: {}", e))
        })
    }

    fn decode_args(&self, args: Decoder) -> transport::Result<Vec<Val>> {
        decode_vals(args, &[Type::String]).map_err(|e| {
            transport::Error::Io(format!("Failed to decode arg: {}", e))
        })
    }

    fn validate_ping(&self, args: &[Val]) -> transport::Result<()> {
        if args.len() != 1 {
            return Err(transport::Error::Io("Expected exactly one argument".into()));
        }
        match &args[0] {
            Val::String(s) if s == "ping" => Ok(()),
            _ => Err(transport::Error::Io("Expected 'ping' string argument".into()))
        }
    }

    fn encode_pong(&self, seq: u64) -> transport::Result<Vec<u8>> {
        let mut enc = Encoder::new();
        let results_bytes = encode_vals_to_bytes(&[Val::String("pong".into())]).map_err(|e| {
            transport::Error::Io(format!("Failed to encode results: {}", e))
        })?;
        ReplyOkEncoder::new(seq, &results_bytes).encode(&mut enc).map_err(|e| {
            transport::Error::Io(format!("Failed to encode reply: {}", e))
        })?;
        enc.into_bytes().map_err(|e| {
            transport::Error::Io(format!("Failed to finalize bytes: {}", e))
        })
    }
}

#[async_trait::async_trait]
impl Transport for PingPongTransport {
    async fn send(&self, payload: &[u8]) -> transport::Result<()> {
        let frame = self.decode_frame(payload)?;

        let response = match frame {
            RpcFrame::Call(call) => {
                let args = self.decode_args(call.args)?;
                self.validate_ping(&args)?;
                self.encode_pong(call.seq)?
            }
            RpcFrame::Reply(_) => {
                return Err(transport::Error::Io("Received Reply frame in transport".into()));
            }
        };

        *self.pending.lock().await = Some(response);
        Ok(())
    }

    async fn recv(&self) -> transport::Result<Option<Vec<u8>>> {
        Ok(self.pending.lock().await.take())
    }
}

/// Mock transport that returns a Call frame instead of Reply (protocol violation).
/// Note: Hardcoded `seq = 999`.
struct CallReturningTransport {
    pending: Arc<Mutex<Option<Vec<u8>>>>,
}

impl CallReturningTransport {
    fn new() -> Self {
        Self {
            pending: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait::async_trait]
impl Transport for CallReturningTransport {
    async fn send(&self, _payload: &[u8]) -> transport::Result<()> {
        let mut enc = Encoder::new();
        let empty_bytes = encode_vals_to_bytes(&[]).unwrap();
        CallEncoder::new(999, "target", "method", &empty_bytes).encode(&mut enc).unwrap();
        let response = enc.into_bytes().unwrap();
        *self.pending.lock().await = Some(response);
        Ok(())
    }

    async fn recv(&self) -> transport::Result<Option<Vec<u8>>> {
        Ok(self.pending.lock().await.take())
    }
}

/// Mock transport that returns a failure reply.
struct FailureTransport {
    pending: Arc<Mutex<Option<Vec<u8>>>>,
}

impl FailureTransport {
    fn new() -> Self {
        Self {
            pending: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait::async_trait]
impl Transport for FailureTransport {
    async fn send(&self, payload: &[u8]) -> transport::Result<()> {
        // Decode to get seq
        let mut dec = Decoder::new(payload);
        let frame = RpcFrame::decode(&mut dec).unwrap();
        let seq = match frame {
            RpcFrame::Call(call) => call.seq,
            _ => 0,
        };

        let mut enc = Encoder::new();
        ReplyErrEncoder::new(seq, FailureReason::AppTrapped).encode(&mut enc).unwrap();
        let response = enc.into_bytes().unwrap();
        *self.pending.lock().await = Some(response);
        Ok(())
    }

    async fn recv(&self) -> transport::Result<Option<Vec<u8>>> {
        Ok(self.pending.lock().await.take())
    }
}

/// Mock transport that always times out.
struct TimeoutTransport;

#[async_trait::async_trait]
impl Transport for TimeoutTransport {
    async fn send(&self, _payload: &[u8]) -> transport::Result<()> {
        Err(transport::Error::Timeout)
    }

    async fn recv(&self) -> transport::Result<Option<Vec<u8>>> {
        Err(transport::Error::Timeout)
    }
}

/// Mock transport that returns malformed bytes.
struct MalformedTransport {
    pending: Arc<Mutex<Option<Vec<u8>>>>,
}

impl MalformedTransport {
    fn new() -> Self {
        Self {
            pending: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait::async_trait]
impl Transport for MalformedTransport {
    async fn send(&self, _payload: &[u8]) -> transport::Result<()> {
        *self.pending.lock().await = Some(vec![0xFF, 0xFF, 0xFF]);
        Ok(())
    }

    async fn recv(&self) -> transport::Result<Option<Vec<u8>>> {
        Ok(self.pending.lock().await.take())
    }
}

#[tokio::test]
async fn test_successful_ping_pong() {
    let transport = Box::new(PingPongTransport::new());
    let peer = Peer::new("test-peer", transport);

    let args = vec![Val::String("ping".into())];
    let expected_types = vec![Type::String];

    // We expect 1 return value: "pong"
    let results = peer.call("target", "method", &args, expected_types)
        .await
        .expect("Call failed");

    assert_eq!(results.len(), 1);
    match &results[0] {
        Val::String(s) => assert_eq!(s, "pong"),
        _ => panic!("Expected String result"),
    }
}

#[tokio::test]
async fn test_transport_error() {
    let transport = Box::new(TimeoutTransport);
    let peer = Peer::new("test-peer", transport);

    let args = vec![Val::U32(42)];
    let expected_types = vec![Type::U32];

    let err = peer.call("target", "method", &args, expected_types).await.unwrap_err();

    match err {
        peer::Error::Transport(transport::Error::Timeout) => {},
        _ => panic!("Expected Transport(Timeout), got {:?}", err),
    }
}

#[tokio::test]
async fn test_rpc_protocol_violation_call_frame() {
    let transport = Box::new(CallReturningTransport::new());
    let peer = Peer::new("test-peer", transport);

    let args = vec![Val::U32(42)];
    let expected_types = vec![Type::U32];

    let err = peer.call("target", "method", &args, expected_types).await.unwrap_err();

    // Protocol violations are now forwarded as the actual error
    match err {
        peer::Error::NeoRpc(neorpc::Error::ProtocolViolation(_)) => {},
        _ => panic!("Expected NeoRpc(ProtocolViolation), got {:?}", err),
    }
}

#[tokio::test]
async fn test_rpc_malformed_frame() {
    let transport = Box::new(MalformedTransport::new());
    let peer = Peer::new("test-peer", transport);

    let args = vec![Val::U32(42)];
    let expected_types = vec![Type::U32];

    let err = peer.call("target", "method", &args, expected_types).await.unwrap_err();

    // Malformed frames are now forwarded as the actual decoding error
    match err {
        peer::Error::NeoRpc(_) | peer::Error::NeoPack(_) => {},
        _ => panic!("Expected NeoRpc or NeoPack error, got {:?}", err),
    }
}

#[tokio::test]
async fn test_remote_failure() {
    let transport = Box::new(FailureTransport::new());
    let peer = Peer::new("test-peer", transport);

    let args = vec![Val::U32(42)];
    let expected_types = vec![Type::U32];

    let err = peer.call("target", "method", &args, expected_types).await.unwrap_err();

    match err {
        peer::Error::Remote(FailureReason::AppTrapped) => {},
        _ => panic!("Expected Remote(AppTrapped), got {:?}", err),
    }
}

/// Concurrent Correlation Test: Verify that the async pump correctly correlates
/// responses to requests even when responses arrive out of order.
#[tokio::test]
async fn test_concurrent_correlation() {
    use neopack::Encoder;
    use neorpc::encode_vals_to_bytes;
    use tokio::sync::mpsc;

    // Create a duplex channel transport pair
    let (client_tx, mut server_rx) = mpsc::unbounded_channel();
    let (server_tx, client_rx) = mpsc::unbounded_channel();

    let client_transport = Box::new(DuplexChannelTransport::new(client_tx, client_rx));
    let peer = Arc::new(Peer::new("test-peer", client_transport));

    // Spawn 10 concurrent peer tasks
    let mut tasks = Vec::new();
    for i in 0..10 {
        let peer = peer.clone();
        let task = tokio::spawn(async move {
            let args = vec![Val::U32(i)];
            let result_types = vec![Type::U32];
            let results = peer.call("target", "method", &args, result_types).await.unwrap();

            // Verify we got the right response
            let expected = 1;
            let actual = results.len();
            assert_eq!(expected, actual);
            match &results[0] {
                Val::U32(v) => assert_eq!(*v, i * 2), // Server doubles the input
                _ => panic!("Expected U32"),
            }
        });
        tasks.push(task);
    }

    // Server side: collect all requests
    let mut requests = Vec::new();
    for _ in 0..10 {
        if let Some(req_bytes) = server_rx.recv().await {
            requests.push(req_bytes);
        }
    }

    // Shuffle and respond in random order
    use rand::seq::SliceRandom;
    use rand::thread_rng;
    requests.shuffle(&mut thread_rng());

    for req_bytes in requests {
        // Decode request to get seq and input value
        let mut dec = Decoder::new(&req_bytes);
        let frame = RpcFrame::decode(&mut dec).unwrap();

        if let RpcFrame::Call(call) = frame {
            // Extract input value
            let args = decode_vals(call.args, &[Type::U32]).unwrap();
            let input = match &args[0] {
                Val::U32(v) => *v,
                _ => panic!("Expected U32"),
            };

            // Send response with doubled value
            let results = vec![Val::U32(input * 2)];
            let results_bytes = encode_vals_to_bytes(&results).unwrap();

            let mut enc = Encoder::new();
            ReplyOkEncoder::new(call.seq, &results_bytes).encode(&mut enc).unwrap();
            let reply_bytes = enc.into_bytes().unwrap();

            server_tx.send(reply_bytes).unwrap();
        }
    }

    // Wait for all client tasks to complete
    for task in tasks {
        task.await.unwrap();
    }
}

/// Failure Fidelity Test: Verify that DomainSpecific errors are propagated correctly.
#[tokio::test]
async fn test_failure_fidelity() {
    use tokio::sync::Mutex;

    struct DomainErrorTransport {
        pending: Arc<Mutex<Option<Vec<u8>>>>,
    }

    impl DomainErrorTransport {
        fn new() -> Self {
            Self {
                pending: Arc::new(Mutex::new(None)),
            }
        }
    }

    #[async_trait::async_trait]
    impl Transport for DomainErrorTransport {
        async fn send(&self, payload: &[u8]) -> transport::Result<()> {
            // Decode to get seq
            let mut dec = Decoder::new(payload);
            let frame = RpcFrame::decode(&mut dec).unwrap();
            let seq = match frame {
                RpcFrame::Call(call) => call.seq,
                _ => 0,
            };

            // Send back a DomainSpecific error
            let mut enc = Encoder::new();
            ReplyErrEncoder::new(seq, FailureReason::DomainSpecific(42, "Auth failed".into()))
                .encode(&mut enc)
                .unwrap();
            let response = enc.into_bytes().unwrap();
            *self.pending.lock().await = Some(response);
            Ok(())
        }

        async fn recv(&self) -> transport::Result<Option<Vec<u8>>> {
            Ok(self.pending.lock().await.take())
        }
    }

    let transport = Box::new(DomainErrorTransport::new());
    let peer = Peer::new("test-peer", transport);

    let args = vec![Val::U32(1)];
    let result_types = vec![Type::U32];

    let err = peer.call("target", "method", &args, result_types).await.unwrap_err();

    // Verify the error is exactly what we sent
    match err {
        peer::Error::Remote(FailureReason::DomainSpecific(code, msg)) => {
            assert_eq!(code, 42);
            assert_eq!(msg, "Auth failed");
        }
        _ => panic!("Expected Remote(DomainSpecific), got {:?}", err),
    }
}
