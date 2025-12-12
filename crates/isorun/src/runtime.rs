//! The main Runtime registry for apps, peers, and WIT definitions

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;
use wasmtime::component::Component;
use wasmtime::Engine;

use crate::handles::AppId;
use crate::handles::PeerId;
use crate::traits::Transport;

#[derive(Clone)]
pub struct Runtime {
    pub(crate) inner: Arc<RuntimeInner>,
}

pub(crate) struct RuntimeInner {
    pub(crate) engine: Engine,
    pub(crate) apps: Mutex<HashMap<AppId, Component>>,
    // Persistent connections to other machines
    pub(crate) peers: Mutex<HashMap<PeerId, Arc<dyn Transport>>>,
    pub(crate) next_id: std::sync::atomic::AtomicU64,
}

impl Runtime {
    pub fn new() -> Result<Self> {
        let mut config = wasmtime::Config::new();
        config.async_support(true);
        config.wasm_component_model(true);
        Ok(Self {
            inner: Arc::new(RuntimeInner {
                engine: Engine::new(&config)?,
                apps: Mutex::new(HashMap::new()),
                peers: Mutex::new(HashMap::new()),
                next_id: std::sync::atomic::AtomicU64::new(1),
            }),
        })
    }

    /// Register a Wasm component (App).
    pub async fn register_app(&self, _name: &str, bytes: &[u8]) -> Result<AppId> {
        let component = Component::new(&self.inner.engine, bytes)?;
        let id = AppId(self.inner.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        self.inner.apps.lock().await.insert(id, component);
        Ok(id)
    }

    /// Add a persistent peer (Transport) to the runtime.
    /// Returns a handle that can be used to link imports to this peer.
    pub async fn add_peer(&self, transport: impl Transport) -> Result<PeerId> {
        let id = PeerId(self.inner.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        self.inner.peers.lock().await.insert(id, Arc::new(transport));
        Ok(id)
    }

    /// Handle an incoming binary payload from a Transport.
    ///
    /// 1. Payload header contains Target RemoteID.
    /// 2. Look up local instance associated with that RemoteID.
    /// 3. Exec.
    pub async fn handle_incoming_rpc(&self, _payload: &[u8]) -> Result<Vec<u8>> {
        // Implementation note: Decode payload header to find the target instance ID,
        // lookup the instance in a registry, and execute the call with canonical ABI.
        todo!("Implement canonical ABI lowering for incoming requests")
    }
}
