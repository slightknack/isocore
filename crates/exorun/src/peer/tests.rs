//! Comprehensive tests for peer lifecycle, reconnection, and configuration.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::sync::{mpsc, Mutex, Notify};
use tokio::time::timeout;
use wasmtime::component::{Type, Val};

use crate::transport::{self, Transport};
use super::{Peer, PeerConfig, PeerState, Error};

// =============================================================================
// Test Transports
// =============================================================================

/// A simple echo transport that responds to calls with success.
struct EchoTransport {
    pending: Arc<Mutex<Option<Vec<u8>>>>,
}

impl EchoTransport {
    fn new() -> Self {
        Self {
            pending: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait::async_trait]
impl Transport for EchoTransport {
    async fn send(&self, payload: &[u8]) -> transport::Result<()> {
        use neopack::{Decoder, Encoder};
        use neorpc::{RpcFrame, ReplyOkEncoder, encode_vals_to_bytes};
        
        let mut dec = Decoder::new(payload);
        let frame = RpcFrame::decode(&mut dec)
            .map_err(|e| transport::Error::Io(e.to_string()))?;
        
        let seq = match frame {
            RpcFrame::Call(c) => c.seq,
            _ => return Err(transport::Error::Io("Expected Call".into())),
        };
        
        let mut enc = Encoder::new();
        let results = encode_vals_to_bytes(&[Val::String("echo".into())])
            .map_err(|e| transport::Error::Io(e.to_string()))?;
        ReplyOkEncoder::new(seq, &results)
            .encode(&mut enc)
            .map_err(|e| transport::Error::Io(e.to_string()))?;
        
        *self.pending.lock().await = Some(enc.into_bytes()
            .map_err(|e| transport::Error::Io(e.to_string()))?);
        Ok(())
    }

    async fn recv(&self) -> transport::Result<Option<Vec<u8>>> {
        Ok(self.pending.lock().await.take())
    }
}

/// Transport that never responds (for timeout testing).
struct HangingTransport {
    notify: Arc<Notify>,
}

impl HangingTransport {
    fn new() -> (Self, Arc<Notify>) {
        let notify = Arc::new(Notify::new());
        (Self { notify: notify.clone() }, notify)
    }
}

#[async_trait::async_trait]
impl Transport for HangingTransport {
    async fn send(&self, _payload: &[u8]) -> transport::Result<()> {
        Ok(())
    }

    async fn recv(&self) -> transport::Result<Option<Vec<u8>>> {
        // Wait forever (or until notified for cleanup)
        self.notify.notified().await;
        Ok(None)
    }
}

/// Transport that immediately returns EOF.
struct EofTransport;

#[async_trait::async_trait]
impl Transport for EofTransport {
    async fn send(&self, _payload: &[u8]) -> transport::Result<()> {
        Err(transport::Error::ConnectionLost("Already closed".into()))
    }

    async fn recv(&self) -> transport::Result<Option<Vec<u8>>> {
        Ok(None) // EOF
    }
}

/// Transport that fails on send.
struct FailingSendTransport;

#[async_trait::async_trait]
impl Transport for FailingSendTransport {
    async fn send(&self, _payload: &[u8]) -> transport::Result<()> {
        Err(transport::Error::Io("Simulated send failure".into()))
    }

    async fn recv(&self) -> transport::Result<Option<Vec<u8>>> {
        // Just hang
        std::future::pending().await
    }
}

/// A controllable transport for testing lifecycle scenarios.
struct ControllableTransport {
    tx: mpsc::UnboundedSender<Vec<u8>>,
    rx: Arc<Mutex<mpsc::UnboundedReceiver<Vec<u8>>>>,
    closed: Arc<AtomicBool>,
}

impl ControllableTransport {
    fn new() -> (Self, ControllableController) {
        let (out_tx, out_rx) = mpsc::unbounded_channel();
        let (in_tx, in_rx) = mpsc::unbounded_channel();
        let closed = Arc::new(AtomicBool::new(false));
        
        let transport = Self {
            tx: out_tx,
            rx: Arc::new(Mutex::new(in_rx)),
            closed: closed.clone(),
        };
        
        let controller = ControllableController {
            closed,
            inject_tx: in_tx,
            _outbound_rx: out_rx,
        };
        
        (transport, controller)
    }
}

#[async_trait::async_trait]
impl Transport for ControllableTransport {
    async fn send(&self, payload: &[u8]) -> transport::Result<()> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(transport::Error::ConnectionLost("Transport closed".into()));
        }
        self.tx.send(payload.to_vec())
            .map_err(|_| transport::Error::ConnectionLost("Channel closed".into()))
    }

    async fn recv(&self) -> transport::Result<Option<Vec<u8>>> {
        if self.closed.load(Ordering::SeqCst) {
            return Ok(None);
        }
        let mut rx = self.rx.lock().await;
        match rx.recv().await {
            Some(data) => Ok(Some(data)),
            None => Ok(None),
        }
    }
}

/// Controller for manipulating the transport from tests.
struct ControllableController {
    closed: Arc<AtomicBool>,
    #[allow(dead_code)]
    inject_tx: mpsc::UnboundedSender<Vec<u8>>,
    _outbound_rx: mpsc::UnboundedReceiver<Vec<u8>>,
}

impl ControllableController {
    /// Simulate transport disconnection.
    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }
    
    /// Inject a message as if received from the network.
    fn _inject_response(&self, data: Vec<u8>) {
        let _ = self.inject_tx.send(data);
    }
}

// =============================================================================
// Lifecycle Tests
// =============================================================================

#[tokio::test]
async fn test_peer_initial_state_is_connected() {
    let transport = Box::new(EchoTransport::new());
    let peer = Peer::new("test", transport, PeerConfig::default());
    
    assert_eq!(peer.state(), PeerState::Connected);
}

#[tokio::test]
async fn test_peer_state_after_transport_eof() {
    let transport = Box::new(EofTransport);
    let peer = Peer::new("test", transport, PeerConfig::default());
    
    // Give the pump task time to notice EOF
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    assert_eq!(peer.state(), PeerState::Disconnected);
}

#[tokio::test]
async fn test_peer_state_after_shutdown() {
    let transport = Box::new(EchoTransport::new());
    let peer = Peer::new("test", transport, PeerConfig::default());
    
    assert_eq!(peer.state(), PeerState::Connected);
    
    peer.shutdown().await;
    
    assert_eq!(peer.state(), PeerState::Shutdown);
}

#[tokio::test]
async fn test_shutdown_notifies_pending_requests() {
    let (hanging, _notify) = HangingTransport::new();
    let peer = Arc::new(Peer::new("test", Box::new(hanging), PeerConfig::default()));
    
    // Start a call that will hang
    let peer_clone = peer.clone();
    let call_task = tokio::spawn(async move {
        peer_clone.call("target", "method", &[], vec![Type::String]).await
    });
    
    // Give it time to register the pending request
    tokio::time::sleep(Duration::from_millis(10)).await;
    
    // Shutdown should notify the pending request
    peer.shutdown().await;
    
    // The call should return an error, not hang
    let result = timeout(Duration::from_millis(100), call_task).await;
    assert!(result.is_ok(), "Call should complete after shutdown");
    
    let call_result = result.unwrap().unwrap();
    assert!(matches!(call_result, Err(Error::Shutdown)));
}

#[tokio::test]
async fn test_shutdown_is_idempotent() {
    let transport = Box::new(EchoTransport::new());
    let peer = Peer::new("test", transport, PeerConfig::default());
    
    peer.shutdown().await;
    peer.shutdown().await; // Should not panic
    peer.shutdown().await; // Should not panic
    
    assert_eq!(peer.state(), PeerState::Shutdown);
}

#[tokio::test]
async fn test_call_after_shutdown_returns_error() {
    let transport = Box::new(EchoTransport::new());
    let peer = Peer::new("test", transport, PeerConfig::default());
    
    peer.shutdown().await;
    
    let result = peer.call("target", "method", &[], vec![]).await;
    assert!(matches!(result, Err(Error::Shutdown)));
}

// =============================================================================
// Reconnection Tests
// =============================================================================

#[tokio::test]
async fn test_reconnect_replaces_transport() {
    let (transport1, controller1) = ControllableTransport::new();
    let peer = Peer::new("test", Box::new(transport1), PeerConfig::default());
    
    // Simulate disconnect
    controller1.close();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(peer.state(), PeerState::Disconnected);
    
    // Reconnect with new transport
    let transport2 = Box::new(EchoTransport::new());
    peer.reconnect(transport2).await.expect("Reconnect should succeed");
    
    assert_eq!(peer.state(), PeerState::Connected);
    
    // Verify calls work
    let result = peer.call("target", "method", &[], vec![Type::String]).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_reconnect_on_shutdown_peer_fails() {
    let transport = Box::new(EchoTransport::new());
    let peer = Peer::new("test", transport, PeerConfig::default());
    
    peer.shutdown().await;
    
    let new_transport = Box::new(EchoTransport::new());
    let result = peer.reconnect(new_transport).await;
    
    assert!(matches!(result, Err(Error::Shutdown)));
}

#[tokio::test]
async fn test_reconnect_while_connected_fails() {
    let transport = Box::new(EchoTransport::new());
    let peer = Peer::new("test", transport, PeerConfig::default());
    
    assert_eq!(peer.state(), PeerState::Connected);
    
    let new_transport = Box::new(EchoTransport::new());
    let result = peer.reconnect(new_transport).await;
    
    // Should fail because already connected
    assert!(matches!(result, Err(Error::AlreadyConnected)));
}

#[tokio::test]
async fn test_reconnect_resets_state_to_connected() {
    let transport = Box::new(EofTransport);
    let peer = Peer::new("test", transport, PeerConfig::default());
    
    // Wait for disconnect
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(peer.state(), PeerState::Disconnected);
    
    // Reconnect
    let new_transport = Box::new(EchoTransport::new());
    peer.reconnect(new_transport).await.expect("Reconnect should succeed");
    
    assert_eq!(peer.state(), PeerState::Connected);
}

// =============================================================================
// Configuration Tests
// =============================================================================

#[tokio::test]
async fn test_custom_timeout_respected() {
    let (hanging, notify) = HangingTransport::new();
    let config = PeerConfig {
        call_timeout: Duration::from_millis(100),
        ..Default::default()
    };
    let peer = Peer::new("test", Box::new(hanging), config);
    
    let start = std::time::Instant::now();
    let result = peer.call("target", "method", &[], vec![]).await;
    let elapsed = start.elapsed();
    
    assert!(matches!(result, Err(Error::Timeout)));
    assert!(elapsed >= Duration::from_millis(100));
    assert!(elapsed < Duration::from_millis(200)); // Should not take much longer
    
    // Cleanup
    notify.notify_one();
}

#[tokio::test]
async fn test_default_timeout_is_30_seconds() {
    let config = PeerConfig::default();
    assert_eq!(config.call_timeout, Duration::from_secs(30));
}

#[tokio::test]
async fn test_call_with_timeout_override() {
    let (hanging, notify) = HangingTransport::new();
    let config = PeerConfig {
        call_timeout: Duration::from_secs(30), // Default is long
        ..Default::default()
    };
    let peer = Peer::new("test", Box::new(hanging), config);
    
    let start = std::time::Instant::now();
    let result = peer.call_with_timeout(
        "target", 
        "method", 
        &[], 
        vec![],
        Duration::from_millis(50), // Override with short timeout
    ).await;
    let elapsed = start.elapsed();
    
    assert!(matches!(result, Err(Error::Timeout)));
    assert!(elapsed >= Duration::from_millis(50));
    assert!(elapsed < Duration::from_millis(150));
    
    notify.notify_one();
}

#[tokio::test]
async fn test_max_pending_rejects_excess() {
    let (hanging, notify) = HangingTransport::new();
    let config = PeerConfig {
        call_timeout: Duration::from_secs(30),
        max_pending: 2,
    };
    let peer = Arc::new(Peer::new("test", Box::new(hanging), config));
    
    // Start two calls that will hang (filling the limit)
    let p1 = peer.clone();
    let p2 = peer.clone();
    let _t1 = tokio::spawn(async move { p1.call("t", "m", &[], vec![]).await });
    let _t2 = tokio::spawn(async move { p2.call("t", "m", &[], vec![]).await });
    
    // Give them time to register
    tokio::time::sleep(Duration::from_millis(20)).await;
    
    // Third call should be rejected immediately
    let result = peer.call("t", "m", &[], vec![]).await;
    assert!(matches!(result, Err(Error::TooManyPendingRequests { limit: 2 })));
    
    notify.notify_one();
}

// =============================================================================
// Zombie Peer Bug Tests
// =============================================================================

#[tokio::test]
async fn test_call_after_pump_death_returns_disconnected_not_timeout() {
    let transport = Box::new(EofTransport);
    let config = PeerConfig {
        call_timeout: Duration::from_secs(5), // Long timeout
        ..Default::default()
    };
    let peer = Peer::new("test", transport, config);
    
    // Wait for pump to die (EOF)
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    // Call should fail fast with Disconnected, not wait for timeout
    let start = std::time::Instant::now();
    let result = peer.call("target", "method", &[], vec![]).await;
    let elapsed = start.elapsed();
    
    assert!(matches!(result, Err(Error::Disconnected)));
    assert!(elapsed < Duration::from_millis(100)); // Should be fast, not 5 seconds
}

#[tokio::test]
async fn test_pump_death_updates_state_to_disconnected() {
    let transport = Box::new(EofTransport);
    let peer = Peer::new("test", transport, PeerConfig::default());
    
    // Initially connected
    assert_eq!(peer.state(), PeerState::Connected);
    
    // Wait for pump to process EOF
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    // Should be disconnected
    assert_eq!(peer.state(), PeerState::Disconnected);
}

// =============================================================================
// Concurrent Tests
// =============================================================================

#[tokio::test]
async fn test_concurrent_calls_all_complete() {
    use neopack::{Decoder, Encoder};
    use neorpc::{RpcFrame, ReplyOkEncoder, decode_vals, encode_vals_to_bytes};
    
    let (client_tx, mut server_rx) = mpsc::unbounded_channel();
    let (server_tx, client_rx) = mpsc::unbounded_channel();
    
    struct DuplexTransport {
        tx: mpsc::UnboundedSender<Vec<u8>>,
        rx: Arc<Mutex<mpsc::UnboundedReceiver<Vec<u8>>>>,
    }
    
    #[async_trait::async_trait]
    impl Transport for DuplexTransport {
        async fn send(&self, payload: &[u8]) -> transport::Result<()> {
            self.tx.send(payload.to_vec())
                .map_err(|_| transport::Error::ConnectionLost("closed".into()))
        }
        async fn recv(&self) -> transport::Result<Option<Vec<u8>>> {
            Ok(self.rx.lock().await.recv().await)
        }
    }
    
    let transport = Box::new(DuplexTransport {
        tx: client_tx,
        rx: Arc::new(Mutex::new(client_rx)),
    });
    let peer = Arc::new(Peer::new("test", transport, PeerConfig::default()));
    
    // Spawn 10 concurrent calls
    let mut tasks = Vec::new();
    for i in 0u32..10 {
        let p = peer.clone();
        tasks.push(tokio::spawn(async move {
            let args = vec![Val::U32(i)];
            let result = p.call("target", "method", &args, vec![Type::U32]).await?;
            match &result[0] {
                Val::U32(v) => Ok(*v),
                _ => Err(Error::ChannelClosed),
            }
        }));
    }
    
    // Server: respond to all (in order for simplicity)
    for _ in 0..10 {
        let req = server_rx.recv().await.unwrap();
        let mut dec = Decoder::new(&req);
        let frame = RpcFrame::decode(&mut dec).unwrap();
        if let RpcFrame::Call(call) = frame {
            let args = decode_vals(call.args, &[Type::U32]).unwrap();
            let input = match &args[0] { Val::U32(v) => *v, _ => 0 };
            
            let mut enc = Encoder::new();
            let results = encode_vals_to_bytes(&[Val::U32(input * 2)]).unwrap();
            ReplyOkEncoder::new(call.seq, &results).encode(&mut enc).unwrap();
            server_tx.send(enc.into_bytes().unwrap()).unwrap();
        }
    }
    
    // Verify all calls completed with correct values
    for (i, task) in tasks.into_iter().enumerate() {
        let result = task.await.unwrap().unwrap();
        assert_eq!(result, (i as u32) * 2);
    }
}

#[tokio::test]
async fn test_shutdown_during_active_calls() {
    let (hanging, notify) = HangingTransport::new();
    let peer = Arc::new(Peer::new("test", Box::new(hanging), PeerConfig::default()));
    
    // Start several calls
    let mut tasks = Vec::new();
    for _ in 0..5 {
        let p = peer.clone();
        tasks.push(tokio::spawn(async move {
            p.call("t", "m", &[], vec![]).await
        }));
    }
    
    // Give them time to register
    tokio::time::sleep(Duration::from_millis(20)).await;
    
    // Shutdown while calls are pending
    peer.shutdown().await;
    
    // All calls should complete with Shutdown error
    for task in tasks {
        let result = task.await.unwrap();
        assert!(matches!(result, Err(Error::Shutdown)));
    }
    
    notify.notify_one();
}

// =============================================================================
// Send Failure Tests
// =============================================================================

#[tokio::test]
async fn test_send_failure_removes_pending_and_returns_error() {
    let transport = Box::new(FailingSendTransport);
    let peer = Peer::new("test", transport, PeerConfig::default());
    
    let result = peer.call("target", "method", &[], vec![]).await;
    
    assert!(matches!(result, Err(Error::Transport(_))));
}

// =============================================================================
// Successful Call Tests
// =============================================================================

#[tokio::test]
async fn test_successful_call_returns_result() {
    let transport = Box::new(EchoTransport::new());
    let peer = Peer::new("test", transport, PeerConfig::default());
    
    let result = peer.call("target", "method", &[], vec![Type::String]).await;
    
    assert!(result.is_ok());
    let vals = result.unwrap();
    assert_eq!(vals.len(), 1);
    match &vals[0] {
        Val::String(s) => assert_eq!(s, "echo"),
        _ => panic!("Expected string"),
    }
}

#[tokio::test]
async fn test_peer_name_is_preserved() {
    let transport = Box::new(EchoTransport::new());
    let peer = Peer::new("my-special-peer", transport, PeerConfig::default());
    
    assert_eq!(peer.peer_name(), "my-special-peer");
}
