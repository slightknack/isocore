use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use wasmtime::component::Linker;

use isorun::ContextBuilder;
use isorun::InstanceBuilder;
use isorun::IsorunCtx;
use isorun::Linkable;
use isorun::RemoteAddr;
use isorun::Runtime;
use isorun::SystemComponent;
use isorun::Transport;

// --- Helper to load Wasm ---
fn wasm(name: &str) -> Vec<u8> {
    let path = format!("tests/fixtures/{}.wasm", name);
    std::fs::read(&path).expect(&format!("Could not read wasm: {}", path))
}

// --- Test 1: Basic Runtime Creation ---

#[tokio::test]
async fn test_runtime_creation() -> anyhow::Result<()> {
    let _rt = Runtime::new()?;
    Ok(())
}

// --- Test 2: App Registration ---

#[tokio::test]
async fn test_app_registration() -> anyhow::Result<()> {
    let rt = Runtime::new()?;
    let _app_id = rt.register_app("logger", &wasm("app_logger")).await?;
    Ok(())
}

// --- Test 3: Peer Registration ---

struct MockTransport;

#[async_trait::async_trait]
impl Transport for MockTransport {
    async fn call(&self, _payload: &[u8]) -> anyhow::Result<Vec<u8>> {
        Ok(vec![])
    }
}

#[tokio::test]
async fn test_peer_registration() -> anyhow::Result<()> {
    let rt = Runtime::new()?;
    let _peer_id = rt.add_peer(MockTransport).await?;
    Ok(())
}

// --- Placeholder tests for future implementation ---

// Test 1: System Integration (Logger)
#[derive(Clone)]
struct SysLogger {
    logs: Arc<Mutex<Vec<String>>>,
}

impl SysLogger {
    fn new() -> Self { Self { logs: Arc::new(Mutex::new(Vec::new())) } }
}

impl SystemComponent for SysLogger {
    fn install(&self, linker: &mut Linker<IsorunCtx>) -> anyhow::Result<()> {
        let logs = self.logs.clone();
        
        linker.instance("test:demo/logging")?.func_wrap(
            "log",
            move |_caller: wasmtime::StoreContextMut<'_, IsorunCtx>, (level, msg): (String, String)| {
                logs.lock().unwrap().push(format!("[{}] {}", level, msg));
                Ok(())
            },
        )?;
        
        Ok(())
    }
}

#[tokio::test]
async fn test_system_integration() -> anyhow::Result<()> {
    let rt = Runtime::new()?;
    let logger_sys = SysLogger::new();
    
    let app_id = rt.register_app("logger", &wasm("app_logger")).await?;
    
    let _instance = InstanceBuilder::new(&rt, app_id)
        .link_system("test:demo/logging", logger_sys.clone())
        .instantiate().await?;

    // TODO: Execute and verify
    Ok(())
}

// Test 2: App-to-App Wiring (Local)
#[tokio::test]
async fn test_app_to_app_local() -> anyhow::Result<()> {
    let rt = Runtime::new()?;
    
    let provider_id = rt.register_app("math", &wasm("app_provider")).await?;
    let consumer_id = rt.register_app("client", &wasm("app_consumer")).await?;

    let provider_inst = InstanceBuilder::new(&rt, provider_id).instantiate().await?;

    let _consumer_inst = InstanceBuilder::new(&rt, consumer_id)
        .link("test:demo/math", Linkable::LocalInstance(provider_inst))
        .instantiate().await?;

    // TODO: Execute and verify
    Ok(())
}

// Test 3: Stateful System (In-Memory KV)
#[derive(Clone)]
struct InMemoryKv {
    store: Arc<Mutex<HashMap<String, String>>>,
}

impl InMemoryKv {
    fn new() -> Self { Self { store: Arc::new(Mutex::new(HashMap::new())) } }
}

impl SystemComponent for InMemoryKv {
    fn install(&self, linker: &mut Linker<IsorunCtx>) -> anyhow::Result<()> {
        let store = self.store.clone();
        
        let mut instance = linker.instance("test:demo/kv")?;
        
        // Bind the 'get' function
        instance.func_wrap(
            "get",
            {
                let store = store.clone();
                move |_caller: wasmtime::StoreContextMut<'_, IsorunCtx>, (key,): (String,)| {
                    let value = store.lock().unwrap().get(&key).cloned();
                    Ok((value,))
                }
            },
        )?;
        
        // Bind the 'set' function
        instance.func_wrap(
            "set",
            move |_caller: wasmtime::StoreContextMut<'_, IsorunCtx>, (key, val): (String, String)| {
                store.lock().unwrap().insert(key, val);
                Ok(())
            },
        )?;
        
        Ok(())
    }
}

#[tokio::test]
async fn test_stateful_system() -> anyhow::Result<()> {
    let rt = Runtime::new()?;
    let kv_sys = InMemoryKv::new();

    let app_id = rt.register_app("kv_client", &wasm("app_kv")).await?;

    let _instance = InstanceBuilder::new(&rt, app_id)
        .link_system("test:demo/kv", kv_sys.clone())
        .instantiate().await?;

    // TODO: Execute and verify
    Ok(())
}

// Test 4: Remote Peer (Mock Transport)
#[tokio::test]
async fn test_remote_peer_mock() -> anyhow::Result<()> {
    let rt = Runtime::new()?;
    
    let peer_id = rt.add_peer(MockTransport).await?;
    let app_id = rt.register_app("consumer", &wasm("app_consumer")).await?;

    let _instance = InstanceBuilder::new(&rt, app_id)
        .link_remote("test:demo/math", RemoteAddr {
            peer: peer_id,
            remote_instance: "math-service-on-mars".into()
        }).await?
        .instantiate().await?;

    // TODO: Execute and verify
    Ok(())
}

// Test 5: Diamond Dependency (Shared System)
#[tokio::test]
async fn test_diamond_dependency() -> anyhow::Result<()> {
    let rt = Runtime::new()?;
    let shared_kv = InMemoryKv::new();

    let app_id = rt.register_app("kv_client", &wasm("app_kv")).await?;

    let _inst_a = InstanceBuilder::new(&rt, app_id)
        .link_system("test:demo/kv", shared_kv.clone())
        .instantiate().await?;

    let _inst_b = InstanceBuilder::new(&rt, app_id)
        .link_system("test:demo/kv", shared_kv.clone())
        .instantiate().await?;

    // TODO: Execute and verify shared state
    Ok(())
}
