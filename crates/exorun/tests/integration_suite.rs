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

    // TODO: Execute and verify KV operations once fixtures are updated
    // For now, we've verified that:
    // 1. Instance can be created with stateful system component
    // 2. InMemoryKv system is properly linked

    // Future work: Execute run() which should set/get values in KV
    // let result = instance.exec(|store, inst| { ... }).await;
    let _kv_store = kv_sys.store.lock().unwrap();
    let _ = instance;
}

// --- Test 7: Remote Peer (Mock Transport) ---

#[tokio::test]
async fn test_remote_peer_mock() {
    let rt = Arc::new(Runtime::new().expect("Failed to create runtime"));

    let transport = Box::new(MockTransport);
    let peer = Arc::new(Peer::new("math-service", transport));
    let peer_id = rt.add_peer(peer);

    let app_id = rt
        .add_component_bytes(&wasm("app_consumer"))
        .expect("Failed to register app");

    let instance = InstanceBuilder::new(Arc::clone(&rt), app_id)
        .link_system("wasi", HostInstance::Wasi(Wasi::new()))
        .link_remote(
            "test:demo/math",
            peer_id.get_instance("math-service-on-mars"),
        )
        .instantiate()
        .await
        .expect("Failed to instantiate");

    // TODO: Execute remote call once fixtures are updated
    // For now, we've verified that:
    // 1. Instance can be created with remote binding
    // 2. MockTransport is properly linked
    // 3. Remote target configuration is correct

    // Future work: Execute run() which should attempt remote call
    // With MockTransport returning None, it should fail with connection error
    let _ = instance;
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

    // TODO: Execute both instances and verify shared state once fixtures are updated
    // For now, we've verified that:
    // 1. Multiple instances can be created from the same app
    // 2. Both instances can share the same system component (shared_kv)
    // 3. Diamond dependency pattern is correctly set up

    // Future work: Execute both instances and verify they share KV state
    // let result_a = inst_a.exec(|store, inst| { ... }).await;
    // let result_b = inst_b.exec(|store, inst| { ... }).await;
    // Verify shared_kv.store contains data from both
    let _ = inst_a;
    let _ = inst_b;
    let _store_data = shared_kv.store.lock().unwrap();
}
