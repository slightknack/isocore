//! InstanceBuilder for wiring up imports and creating instances

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use tokio::sync::Mutex;
use wasmtime::component::Linker;
use wasmtime::Store;

use crate::context::Budget;
use crate::context::ContextBuilder;
use crate::context::IsorunCtx;
use crate::handles::AppId;

use crate::handles::RemoteAddr;
use crate::instance::InstanceHandle;
use crate::runtime::Runtime;
use crate::traits::SystemComponent;
use crate::traits::Transport;

/// What are we linking an import to?
#[derive(Clone)]
pub enum Linkable {
    /// A local Rust implementation (System).
    System(Arc<dyn SystemComponent>),

    /// Another running Wasm instance in the same process.
    /// (Direct, fast memory access).
    LocalInstance(InstanceHandle),

    /// A remote instance accessed via a generic Transport.
    /// The Runtime handles the ABI serialization automatically.
    Remote {
        /// The pipe to send bytes through.
        transport: Arc<dyn Transport>,
        /// The opaque ID the remote peer uses to find the target instance
        remote_instance: String,
    },
}

pub struct InstanceBuilder<'a> {
    rt: &'a Runtime,
    app_id: AppId,
    budget: Budget,
    links: HashMap<String, Linkable>,
}

impl<'a> InstanceBuilder<'a> {
    pub fn new(rt: &'a Runtime, app_id: AppId) -> Self {
        Self {
            rt,
            app_id,
            budget: Budget::standard(),
            links: HashMap::new(),
        }
    }

    pub fn budget(mut self, budget: Budget) -> Self {
        self.budget = budget;
        self
    }

    /// Link an import to a generic Linkable.
    pub fn link(mut self, name: &str, target: Linkable) -> Self {
        self.links.insert(name.to_string(), target);
        self
    }

    /// Helper: Link to a specific Remote Address (Peer + Remote ID).
    /// e.g. .link_remote("kv", RemoteAddr { peer: peer_tcp, remote_instance: "kv-primary" })
    pub async fn link_remote(
        mut self,
        name: &str,
        addr: RemoteAddr
    ) -> Result<Self> {
        let peers = self.rt.inner.peers.lock().await;
        let transport = peers.get(&addr.peer).ok_or_else(|| anyhow!("Peer not found"))?;

        self.links.insert(name.to_string(), Linkable::Remote { 
            transport: transport.clone(),
            remote_instance: addr.remote_instance,
        });
        Ok(self)
    }

    /// Helper: Link to a local system implementation.
    pub fn link_system(mut self, name: &str, sys: impl SystemComponent) -> Self {
        self.links.insert(name.to_string(), Linkable::System(Arc::new(sys)));
        self
    }

    /// Instantiate the App.
    pub async fn instantiate(self) -> Result<InstanceHandle> {
        let apps = self.rt.inner.apps.lock().await;
        let component = apps.get(&self.app_id).ok_or_else(|| anyhow!("App not found"))?;

        let mut linker = Linker::<IsorunCtx>::new(&self.rt.inner.engine);
        let mut ctx_builder = ContextBuilder::new();

        // --- The Wiring Loop ---
        for (_name, target) in self.links {
            match target {
                Linkable::System(sys) => {
                    // 1. Install definitions
                    sys.install(&mut linker)?;
                    // 2. Configure context (WASI preopens, Auth, etc)
                    sys.configure(&mut ctx_builder)?;
                }
                Linkable::LocalInstance(_handle) => {
                    // Bridge local instances directly via host functions
                    // Implementation note: uses linker.func_wrap_async
                }
                Linkable::Remote { transport: _, remote_instance: _ } => {
                    // Generate RPC stubs that serialize calls to bytes
                    // using the component's import definitions
                    // Implementation note:
                    //   a. Inspect component imports to get function signatures
                    //   b. Generate host function that serializes args to Canonical ABI
                    //   c. Prepends `remote_instance` to the payload
                    //   d. transport.call(Bytes).await
                    //   e. Deserializes response and lowers to Wasm
                }
            }
        }

        // Finalize Context
        let ctx = IsorunCtx::new(ctx_builder);

        let mut store = Store::new(&self.rt.inner.engine, ctx);
        // TODO: Apply budget logic - would need to implement StoreLimits trait

        let instance = linker.instantiate_async(&mut store, component).await?;

        Ok(InstanceHandle {
            store: Arc::new(Mutex::new(store)),
            instance,
        })
    }
}
