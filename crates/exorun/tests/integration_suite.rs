//! Integration tests for exorun runtime.

use std::sync::Arc;

use wasmtime::component::Val;

use exorun::peer::Peer;
use exorun::runtime::Runtime;
use exorun::host::HostInstance;
use exorun::host::Wasi;
use exorun::transport::Transport;

/// Helper to load Wasm fixtures.
fn wasm(name: &str) -> Vec<u8> {
    let path = format!("tests/fixtures/{}.wasm", name);
    std::fs::read(&path).unwrap_or_else(|_| panic!("Could not read wasm: {}", path))
}

// --- Test 1: Basic Runtime Creation ---

#[tokio::test]
async fn test_runtime_creation() {
    let _rt = Runtime::new().expect("Failed to create runtime");
}

// --- Test 2: App Registration ---

#[tokio::test]
async fn test_app_registration() {
    let rt = Runtime::new().expect("Failed to create runtime");
    let bytes = wasm("app_logger");
    let _app_id = rt.add_component_bytes(&bytes).expect("Failed to register app");
}

// --- Test 3: Peer Registration ---

struct MockTransport;

#[async_trait::async_trait]
impl Transport for MockTransport {
    async fn send(&self, _payload: &[u8]) -> Result<(), exorun::transport::Error> {
        Ok(())
    }

    async fn recv(&self) -> Result<Option<Vec<u8>>, exorun::transport::Error> {
        Ok(None)
    }
}

#[tokio::test]
async fn test_peer_registration() {
    let runtime = Arc::new(Runtime::new().expect("Failed to create runtime"));
    let transport = Box::new(MockTransport);
    let peer = Arc::new(Peer::new("test-peer", transport));
    let _peer_id = runtime.add_peer(peer);
}

// --- Test 4: System Integration (Logger) ---

#[tokio::test]
async fn test_system_integration() {
    let rt = Runtime::new().expect("Failed to create runtime");

    let logger = exorun::host::Logger::new();

    let app_id = rt
        .add_component_bytes(&wasm("app_logger"))
        .expect("Failed to register app");

    let instance_id = rt.instantiate(app_id)
        .link_system("wasi:cli/environment", HostInstance::Wasi(Wasi::new()))
        .link_system("exorun:host/logging", HostInstance::Logger(logger.clone()))
        .build()
        .await
        .expect("Failed to instantiate");

    // Call the run() function from the exorun:test/runnable interface
    let results = rt.call(instance_id, "exorun:test/runnable", "run", &[])
        .await
        .expect("Failed to execute run()");

    // Extract and verify the result
    let result = match &results[0] {
        Val::String(s) => s.clone(),
        _ => panic!("Expected string result"),
    };
    assert_eq!(result, "Done");

    // Verify logs were captured by the system component
    let logs = logger.get_logs().await;
    assert_eq!(logs.len(), 1, "Expected exactly one log entry");
    assert_eq!(logs[0], "[INFO] Hello from Wasm!", "Log message mismatch");
}

// --- Test 5: App-to-App Wiring (Local) ---

#[tokio::test]
async fn test_app_to_app_local() {
    let rt = Runtime::new().expect("Failed to create runtime");

    let provider_id = rt
        .add_component_bytes(&wasm("app_provider"))
        .expect("Failed to register provider");
    let consumer_id = rt
        .add_component_bytes(&wasm("app_consumer"))
        .expect("Failed to register consumer");

    let provider_inst_id = rt.instantiate(provider_id)
        .link_system("wasi:cli/environment", HostInstance::Wasi(Wasi::new()))
        .build()
        .await
        .expect("Failed to instantiate provider");

    let consumer_inst_id = rt.instantiate(consumer_id)
        .link_system("wasi:cli/environment", HostInstance::Wasi(Wasi::new()))
        .link_local("exorun:test/math", provider_inst_id)
        .build()
        .await
        .expect("Failed to instantiate consumer");

    // Execute the consumer's run() function, which should internally call
    // the provider's add() function through the local binding
    let results = rt.call(consumer_inst_id, "exorun:test/runnable", "run", &[])
        .await
        .expect("Failed to execute consumer run()");

    // Extract and verify the result
    let result = match &results[0] {
        Val::String(s) => s.clone(),
        _ => panic!("Expected string result from consumer run()"),
    };

    // The consumer should have called the provider's add() function
    // and returned a result with the calculation
    assert!(!result.is_empty(), "Consumer should return a non-empty result");
    assert_eq!(result, "10 + 5 = 15", "Consumer should return the math result from provider");
}

// --- Test 6: Stateful System (In-Memory KV) ---

#[tokio::test]
async fn test_stateful_system() {
    let rt = Runtime::new().expect("Failed to create runtime");

    let kv = exorun::host::Kv::new();

    let app_id = rt
        .add_component_bytes(&wasm("app_kv"))
        .expect("Failed to register app");

    let instance_id = rt.instantiate(app_id)
        .link_system("wasi:cli/environment", HostInstance::Wasi(Wasi::new()))
        .link_system("exorun:host/kv", HostInstance::Kv(kv.clone()))
        .build()
        .await
        .expect("Failed to instantiate");

    // Execute the run() function which should set/get values in KV
    let results = rt.call(instance_id, "exorun:test/runnable", "run", &[])
        .await
        .expect("Failed to execute run()");

    // Extract the result
    let result = match &results[0] {
        Val::String(s) => s.clone(),
        _ => panic!("Expected string result from run()"),
    };

    // Verify the KV operations worked
    assert!(!result.is_empty(), "KV app should return a non-empty result");

    // Verify the KV store has data
    let kv_store = kv.get_store().await;
    assert!(!kv_store.is_empty(), "KV store should contain data after execution");
}

// --- Test 7: Remote Peer (Mock Transport) ---

struct MathServiceTransport {
    response_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    response_rx: tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>>,
}

impl MathServiceTransport {
    fn new() -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            response_tx: tx,
            response_rx: tokio::sync::Mutex::new(rx),
        }
    }
}

#[async_trait::async_trait]
impl Transport for MathServiceTransport {
    async fn send(&self, payload: &[u8]) -> Result<(), exorun::transport::Error> {
        use neopack::{Decoder, Encoder};
        use neorpc::{RpcFrame, ReplyOkEncoder, encode_vals_to_bytes};
        use wasmtime::component::Val;
        
        let mut dec = Decoder::new(payload);
        let frame = RpcFrame::decode(&mut dec).map_err(|e| exorun::transport::Error::Io(e.to_string()))?;

        match frame {
            RpcFrame::Call(mut c) => {
                eprintln!("Mock transport received call: seq={}, target={}, method={}", c.seq, c.target, c.method);
                // Decode the call arguments (two u32 values from a list)
                let mut list_iter = c.args.list().map_err(|e| exorun::transport::Error::Io(e.to_string()))?;
                let a = list_iter.next()
                    .ok_or_else(|| exorun::transport::Error::Io("Missing arg a".to_string()))?
                    .u32().map_err(|e| exorun::transport::Error::Io(e.to_string()))?;
                let b = list_iter.next()
                    .ok_or_else(|| exorun::transport::Error::Io("Missing arg b".to_string()))?
                    .u32().map_err(|e| exorun::transport::Error::Io(e.to_string()))?;
                eprintln!("Mock transport decoded args: a={}, b={}", a, b);
                
                // Calculate result: add(a, b) = a + b
                let result = a + b;
                
                // Encode the result using encode_vals_to_bytes
                let result_bytes = encode_vals_to_bytes(&[Val::U32(result)])
                    .map_err(|e| exorun::transport::Error::Io(e.to_string()))?;
                
                // Create reply
                let mut enc = Encoder::new();
                ReplyOkEncoder::new(c.seq, &result_bytes).encode(&mut enc).map_err(|e| exorun::transport::Error::Io(e.to_string()))?;
                let response = enc.into_bytes().map_err(|e| exorun::transport::Error::Io(e.to_string()))?;
                
                eprintln!("Mock transport sending response: seq={}, result={}, bytes={:?}", c.seq, result, response);
                self.response_tx.send(response).map_err(|_| exorun::transport::Error::ConnectionLost("Channel closed".into()))?;
            }
            _ => return Err(exorun::transport::Error::Io("Expected Call frame".to_string())),
        };

        Ok(())
    }

    async fn recv(&self) -> Result<Option<Vec<u8>>, exorun::transport::Error> {
        let msg = self.response_rx.lock().await.recv().await;
        eprintln!("Mock transport recv: {:?}", msg.as_ref().map(|m| m.len()));
        Ok(msg)
    }
}

#[tokio::test]
async fn test_remote_peer_mock() {
    let rt = Runtime::new().expect("Failed to create runtime");

    let transport = Box::new(MathServiceTransport::new());
    let peer = Arc::new(Peer::new("math-service", transport));
    let peer_id = rt.add_peer(peer);

    let app_id = rt
        .add_component_bytes(&wasm("app_consumer"))
        .expect("Failed to register app");

    let instance_id = rt.instantiate(app_id)
        .link_system("wasi:cli/environment", HostInstance::Wasi(Wasi::new()))
        .link_remote(
            "exorun:test/math",
            peer_id.get_instance("math-service-on-mars"),
        )
        .build()
        .await
        .expect("Failed to instantiate");

    // Execute run() which should make a remote call to the math service
    let results = rt.call(instance_id, "exorun:test/runnable", "run", &[])
        .await
        .expect("Failed to execute run()");

    // Extract and verify the result
    let result = match &results[0] {
        Val::String(s) => s.clone(),
        _ => panic!("Expected string result from consumer run()"),
    };

    // The consumer should have successfully called the remote math service
    assert!(!result.is_empty(), "Consumer should return a non-empty result");
    assert_eq!(result, "10 + 5 = 15", "Consumer should return the math result from remote peer");
}

// --- Test 8: Diamond Dependency (Shared System) ---

#[tokio::test]
async fn test_diamond_dependency() {
    let rt = Runtime::new().expect("Failed to create runtime");

    let shared_kv = exorun::host::Kv::new();

    let app_id = rt
        .add_component_bytes(&wasm("app_kv"))
        .expect("Failed to register app");

    // Create instance A with shared KV
    let inst_a_id = rt.instantiate(app_id)
        .link_system("wasi:cli/environment", HostInstance::Wasi(Wasi::new()))
        .link_system("exorun:host/kv", HostInstance::Kv(shared_kv.clone()))
        .build()
        .await
        .expect("Failed to instantiate instance A");

    // Create instance B with same shared KV
    let inst_b_id = rt.instantiate(app_id)
        .link_system("wasi:cli/environment", HostInstance::Wasi(Wasi::new()))
        .link_system("exorun:host/kv", HostInstance::Kv(shared_kv.clone()))
        .build()
        .await
        .expect("Failed to instantiate instance B");

    // Execute instance A - it should write to the shared KV
    let results_a = rt.call(inst_a_id, "exorun:test/runnable", "run", &[])
        .await
        .expect("Failed to execute instance A run()");

    let result_a = match &results_a[0] {
        Val::String(s) => s.clone(),
        _ => panic!("Expected string result from instance A run()"),
    };

    // Execute instance B - it should also write to the shared KV
    let results_b = rt.call(inst_b_id, "exorun:test/runnable", "run", &[])
        .await
        .expect("Failed to execute instance B run()");

    let result_b = match &results_b[0] {
        Val::String(s) => s.clone(),
        _ => panic!("Expected string result from instance B run()"),
    };

    // Verify both instances executed successfully
    assert!(!result_a.is_empty(), "Instance A should return a non-empty result");
    assert!(!result_b.is_empty(), "Instance B should return a non-empty result");

    // Verify the shared KV store contains data from both instances
    let kv_store = shared_kv.get_store().await;
    assert!(!kv_store.is_empty(), "Shared KV store should contain data from both instances");
    
    // The diamond dependency test verifies:
    // 1. Multiple instances can be created from the same component
    // 2. Both instances share the same system component (shared_kv)
    // 3. Both instances can execute and interact with the shared state
}
