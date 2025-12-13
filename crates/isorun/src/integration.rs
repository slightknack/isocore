use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use wasmtime::component::Linker;
use isorun::InstanceBuilder;
use isorun::IsorunCtx;
use isorun::Linkable;
use isorun::RemoteAddr;
use isorun::Runtime;
use isorun::SystemComponent;
use isorun::Transport;

fn wasm(name: &str) -> Vec<u8> {
    let path = format!("tests/fixtures/{}.wasm", name);
    std::fs::read(&path).expect(&format!("Could not read wasm: {}", path))
}

#[tokio::test]
async fn test_runtime_creation() -> anyhow::Result<()> {
    let _rt = Runtime::new()?;
    Ok(())
}

#[tokio::test]
async fn test_app_registration() -> anyhow::Result<()> {
    let rt = Runtime::new()?;
    let _app_id = rt.register_app("logger", &wasm("app_logger")).await?;
    Ok(())
}

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
