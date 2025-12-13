//! The main Runtime registry for apps, peers, and instances.
//!
//! The `Runtime` is the central coordinator for isorun. It maintains:
//! - **Apps**: Registered WebAssembly components (templates)
//! - **Peers**: Persistent Transport connections to other runtimes
//! - **Instances**: Live running instances, addressable by target ID
//!
//! # Lifecycle
//!
//! 1. Create a Runtime
//! 2. Register apps (`register_app`)
//! 3. Add remote peers (`add_peer`)
//! 4. Instantiate apps with `InstanceBuilder`
//! 5. Register instances for incoming RPC (`register_instance`)

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;

use tokio::sync::Mutex;

use wasmtime::Engine;
use wasmtime::component::Component;

use crate::handles::AppId;
use crate::handles::PeerId;
use crate::instance::InstanceHandle;
use crate::traits::Transport;

/// The central runtime registry.
///
/// This is cheap to clone - all clones share the same underlying registry.
#[derive(Clone)]
pub struct Runtime {
    pub(crate) inner: Arc<RuntimeInner>,
}

pub(crate) struct RuntimeInner {
    pub(crate) engine: Engine,
    pub(crate) apps: Mutex<HashMap<AppId, Component>>,
    pub(crate) peers: Mutex<HashMap<PeerId, Arc<dyn Transport>>>,
    pub(crate) instances: Mutex<HashMap<String, InstanceHandle>>,
    pub(crate) next_id: std::sync::atomic::AtomicU64,
}

impl Runtime {
    /// Create a new runtime with default configuration.
    ///
    /// The runtime is configured for:
    /// - Async support (required for isorun)
    /// - Component model (required for WIT interfaces)
    pub fn new() -> Result<Self> {
        let mut config = wasmtime::Config::new();
        config.async_support(true);
        config.wasm_component_model(true);
        Ok(Self {
            inner: Arc::new(RuntimeInner {
                engine: Engine::new(&config)?,
                apps: Mutex::new(HashMap::new()),
                peers: Mutex::new(HashMap::new()),
                instances: Mutex::new(HashMap::new()),
                next_id: std::sync::atomic::AtomicU64::new(1),
            }),
        })
    }

    /// Register a WebAssembly component as an app template.
    ///
    /// The same app can be instantiated multiple times with different configurations.
    ///
    /// # Arguments
    ///
    /// * `_name` - Human-readable name (currently unused, for future debugging)
    /// * `bytes` - The compiled WebAssembly component bytes
    pub async fn register_app(&self, _name: &str, bytes: &[u8]) -> Result<AppId> {
        let component = Component::new(&self.inner.engine, bytes)?;
        let id = AppId(self.inner.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        self.inner.apps.lock().await.insert(id, component);
        Ok(id)
    }

    /// Add a persistent peer connection to the runtime.
    ///
    /// The Transport will be used to send RPCs to instances on that peer.
    ///
    /// # Returns
    ///
    /// A `PeerId` handle that can be used with `InstanceBuilder::link_remote`.
    pub async fn add_peer(&self, transport: impl Transport) -> Result<PeerId> {
        let id = PeerId(self.inner.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        self.inner.peers.lock().await.insert(id, Arc::new(transport));
        Ok(id)
    }

    /// Register a live instance with a target identifier.
    /// This allows the instance to receive incoming RPC calls addressed to this target.
    pub async fn register_instance(&self, target_id: String, handle: InstanceHandle) -> Result<()> {
        self.inner.instances.lock().await.insert(target_id, handle);
        Ok(())
    }

    /// Unregister an instance by its target identifier.
    pub async fn unregister_instance(&self, target_id: &str) -> Result<()> {
        self.inner.instances.lock().await.remove(target_id);
        Ok(())
    }

    /// Get an instance by its target identifier.
    pub async fn get_instance(&self, target_id: &str) -> Result<InstanceHandle> {
        let instances = self.inner.instances.lock().await;
        instances.get(target_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Instance not found: {}", target_id))
    }
}
