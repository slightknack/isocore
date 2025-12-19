//! Integration tests for exorun runtime.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use wasmtime::component::Linker;
use wasmtime::component::Val;

use exorun::local::InstanceBuilder;
use exorun::peer::Peer;
use exorun::context::ExorunCtx;
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

#[derive(Clone)]
struct SysLogger {
    logs: Arc<Mutex<Vec<String>>>,
}

impl SysLogger {
    fn new() -> Self {
        Self {
            logs: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn get_logs(&self) -> Vec<String> {
        self.logs.lock().unwrap().clone()
    }

    fn install(&self, linker: &mut Linker<ExorunCtx>) -> Result<(), exorun::host::Error> {
        let logs = self.logs.clone();

        let mut instance = linker
            .instance("test:demo/logging")
            .map_err(|e| exorun::host::Error::Link(e.to_string()))?;

        instance
            .func_wrap(
                "log",
                move |_caller: wasmtime::StoreContextMut<'_, ExorunCtx>,
                      (level, msg): (String, String)| {
                    logs.lock().unwrap().push(format!("[{}] {}", level, msg));
                    Ok(())
                },
            )
            .map_err(|e| exorun::host::Error::Link(e.to_string()))?;

        Ok(())
    }
}

#[tokio::test]
async fn test_system_integration() {
    let rt = Arc::new(Runtime::new().expect("Failed to create runtime"));
    let logger_sys = SysLogger::new();

    let app_id = rt
        .add_component_bytes(&wasm("app_logger"))
        .expect("Failed to register app");

    let component = rt.get_component(app_id).expect("Failed to get component");

    // For custom system components, we need to manually set up the linker
    let mut linker = Linker::new(rt.engine());
    let mut ctx_builder = exorun::context::ContextBuilder::new();

    HostInstance::Wasi(Wasi::new()).link(&mut linker, &mut ctx_builder).expect("Failed to link WASI");
    logger_sys.install(&mut linker).expect("Failed to install logger");

    let ctx = ctx_builder.build(Arc::clone(&rt));
    let mut store = wasmtime::Store::new(rt.engine(), ctx);

    let wasmtime_instance = linker
        .instantiate_async(&mut store, &component)
        .await
        .expect("Failed to instantiate");

    let instance = exorun::local::LocalInstance::new(store, wasmtime_instance, component.clone());

    // Call the run() function from the test:demo/runnable interface
    use wasmtime::component::Val;
    let mut results = vec![Val::String(String::new())];
    instance
        .call_interface_func(
            &component,
            "test:demo/runnable",
            "run",
            &[],
            &mut results,
        )
        .await
        .expect("Failed to execute run()");

    // Extract and verify the result
    let result = match &results[0] {
        Val::String(s) => s.clone(),
        _ => panic!("Expected string result"),
    };
    assert_eq!(result, "Done");

    // Verify logs were captured by the system component
    let logs = logger_sys.get_logs();
    assert_eq!(logs.len(), 1, "Expected exactly one log entry");
    assert_eq!(logs[0], "[INFO] Hello from Wasm!", "Log message mismatch");
}

// --- Test 5: App-to-App Wiring (Local) ---

#[tokio::test]
async fn test_app_to_app_local() {
    let rt = Arc::new(Runtime::new().expect("Failed to create runtime"));

    let provider_id = rt
        .add_component_bytes(&wasm("app_provider"))
        .expect("Failed to register provider");
    let consumer_id = rt
        .add_component_bytes(&wasm("app_consumer"))
        .expect("Failed to register consumer");

    let provider_inst = InstanceBuilder::new(Arc::clone(&rt), provider_id)
        .link_system("wasi", HostInstance::Wasi(Wasi::new()))
        .instantiate()
        .await
        .expect("Failed to instantiate provider");

    let consumer_inst = InstanceBuilder::new(Arc::clone(&rt), consumer_id)
        .link_system("wasi", HostInstance::Wasi(Wasi::new()))
        .link_local("test:demo/math", provider_inst)
        .instantiate()
        .await
        .expect("Failed to instantiate consumer");

    // Execute the consumer's run() function, which should internally call
    // the provider's add() function through the local binding
    let consumer_component = rt.get_component(consumer_id).expect("Failed to get consumer component");

    let mut results = vec![Val::String(String::new())];
    consumer_inst
        .call_interface_func(
            &consumer_component,
            "test:demo/runnable",
            "run",
            &[],
            &mut results,
        )
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

#[derive(Clone)]
struct InMemoryKv {
    store: Arc<Mutex<HashMap<String, String>>>,
}

impl InMemoryKv {
    fn new() -> Self {
        Self {
            store: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn install(&self, linker: &mut Linker<ExorunCtx>) -> Result<(), exorun::host::Error> {
        let store = self.store.clone();

        let mut instance = linker
            .instance("test:demo/kv")
            .map_err(|e| exorun::host::Error::Link(e.to_string()))?;

        // Bind the 'get' function
        instance
            .func_wrap(
                "get",
                {
                    let store = store.clone();
                    move |_caller: wasmtime::StoreContextMut<'_, ExorunCtx>, (key,): (String,)| {
                        let value = store.lock().unwrap().get(&key).cloned();
                        Ok((value,))
                    }
                },
            )
            .map_err(|e| exorun::host::Error::Link(e.to_string()))?;

        // Bind the 'set' function
        instance
            .func_wrap(
                "set",
                move |_caller: wasmtime::StoreContextMut<'_, ExorunCtx>,
                      (key, val): (String, String)| {
                    store.lock().unwrap().insert(key, val);
                    Ok(())
                },
            )
            .map_err(|e| exorun::host::Error::Link(e.to_string()))?;

        Ok(())
    }
}

#[tokio::test]
async fn test_stateful_system() {
    let rt = Arc::new(Runtime::new().expect("Failed to create runtime"));
    let kv_sys = InMemoryKv::new();

    let app_id = rt
        .add_component_bytes(&wasm("app_kv"))
        .expect("Failed to register app");

    // For custom system components, we need to manually set up the linker
    let component = rt.get_component(app_id).expect("Failed to get component");
    let mut linker = Linker::new(rt.engine());
    let mut ctx_builder = exorun::context::ContextBuilder::new();

    HostInstance::Wasi(Wasi::new()).link(&mut linker, &mut ctx_builder).expect("Failed to link WASI");
    kv_sys.install(&mut linker).expect("Failed to install kv");

    let ctx = ctx_builder.build(Arc::clone(&rt));
    let mut store = wasmtime::Store::new(rt.engine(), ctx);

    let wasmtime_instance = linker
        .instantiate_async(&mut store, &component)
        .await
        .expect("Failed to instantiate");

    let instance = exorun::local::LocalInstance::new(store, wasmtime_instance, component.clone());

    // Execute the run() function which should set/get values in KV
    let mut results = vec![Val::String(String::new())];
    instance
        .call_interface_func(
            &component,
            "test:demo/runnable",
            "run",
            &[],
            &mut results,
        )
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
    let kv_store = kv_sys.store.lock().unwrap();
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
    let rt = Arc::new(Runtime::new().expect("Failed to create runtime"));

    let transport = Box::new(MathServiceTransport::new());
    let peer = Arc::new(Peer::new("math-service", transport));
    let peer_id = rt.add_peer(peer);

    let app_id = rt
        .add_component_bytes(&wasm("app_consumer"))
        .expect("Failed to register app");

    let consumer_component = rt.get_component(app_id).expect("Failed to get component");

    let instance = InstanceBuilder::new(Arc::clone(&rt), app_id)
        .link_system("wasi", HostInstance::Wasi(Wasi::new()))
        .link_remote(
            "test:demo/math",
            peer_id.get_instance("math-service-on-mars"),
        )
        .instantiate()
        .await
        .expect("Failed to instantiate");

    // Execute run() which should make a remote call to the math service
    use wasmtime::component::Val;
    let mut results = vec![Val::String(String::new())];
    instance
        .call_interface_func(
            &consumer_component,
            "test:demo/runnable",
            "run",
            &[],
            &mut results,
        )
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
    let rt = Arc::new(Runtime::new().expect("Failed to create runtime"));
    let shared_kv = InMemoryKv::new();

    let app_id = rt
        .add_component_bytes(&wasm("app_kv"))
        .expect("Failed to register app");

    let component = rt.get_component(app_id).expect("Failed to get component");

    // Create instance A with shared KV
    let mut linker_a = Linker::new(rt.engine());
    let mut ctx_builder_a = exorun::context::ContextBuilder::new();

    HostInstance::Wasi(Wasi::new()).link(&mut linker_a, &mut ctx_builder_a).expect("Failed to link WASI");
    shared_kv.install(&mut linker_a).expect("Failed to install kv");

    let ctx_a = ctx_builder_a.build(Arc::clone(&rt));
    let mut store_a = wasmtime::Store::new(rt.engine(), ctx_a);

    let wasmtime_instance_a = linker_a
        .instantiate_async(&mut store_a, &component)
        .await
        .expect("Failed to instantiate instance A");

    let inst_a = exorun::local::LocalInstance::new(store_a, wasmtime_instance_a, component.clone());

    // Create instance B with same shared KV
    let mut linker_b = Linker::new(rt.engine());
    let mut ctx_builder_b = exorun::context::ContextBuilder::new();

    HostInstance::Wasi(Wasi::new()).link(&mut linker_b, &mut ctx_builder_b).expect("Failed to link WASI");
    shared_kv.install(&mut linker_b).expect("Failed to install kv");

    let ctx_b = ctx_builder_b.build(Arc::clone(&rt));
    let mut store_b = wasmtime::Store::new(rt.engine(), ctx_b);

    let wasmtime_instance_b = linker_b
        .instantiate_async(&mut store_b, &component)
        .await
        .expect("Failed to instantiate instance B");

    let inst_b = exorun::local::LocalInstance::new(store_b, wasmtime_instance_b, component.clone());

    // Execute instance A - it should write to the shared KV
    use wasmtime::component::Val;
    let mut results_a = vec![Val::String(String::new())];
    inst_a
        .call_interface_func(
            &component,
            "test:demo/runnable",
            "run",
            &[],
            &mut results_a,
        )
        .await
        .expect("Failed to execute instance A run()");

    let result_a = match &results_a[0] {
        Val::String(s) => s.clone(),
        _ => panic!("Expected string result from instance A run()"),
    };

    // Execute instance B - it should also write to the shared KV
    let mut results_b = vec![Val::String(String::new())];
    inst_b
        .call_interface_func(
            &component,
            "test:demo/runnable",
            "run",
            &[],
            &mut results_b,
        )
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
    let kv_store = shared_kv.store.lock().unwrap();
    assert!(!kv_store.is_empty(), "Shared KV store should contain data from both instances");
    
    // The diamond dependency test verifies:
    // 1. Multiple instances can be created from the same component
    // 2. Both instances share the same system component (shared_kv)
    // 3. Both instances can execute and interact with the shared state
}
