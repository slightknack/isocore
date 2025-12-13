//! Instance builder for wiring up imports.
//!
//! The `InstanceBuilder` provides a fluent API for configuring and instantiating
//! WebAssembly components with various linking strategies.
//!
//! # Example
//!
//! ```rust,no_run
//! # use isorun::{Runtime, InstanceBuilder, Linkable, Budget};
//! # async fn example(rt: Runtime, app_id: isorun::AppId) -> anyhow::Result<()> {
//! let instance = InstanceBuilder::new(&rt, app_id)
//!     .budget(Budget::standard())
//!     .link_system("wasi:filesystem", MyFilesystem)
//!     .instantiate()
//!     .await?;
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;

use tokio::sync::Mutex;

use wasmtime::Store;
use wasmtime::component::Linker;

use crate::context::Budget;
use crate::context::ContextBuilder;
use crate::context::IsorunCtx;
use crate::handles::AppId;
use crate::handles::RemoteAddr;
use crate::instance::InstanceHandle;
use crate::runtime::Runtime;
use crate::traits::SystemComponent;
use crate::traits::Transport;

/// The three strategies for linking an import.
///
/// Each strategy has different performance and isolation characteristics:
///
/// - **System**: Fastest (native Rust), no isolation
/// - **LocalInstance**: Fast (same process), memory isolation
/// - **Remote**: Slowest (network), full process isolation
#[derive(Clone)]
pub enum Linkable {
    /// A local Rust implementation (System component).
    ///
    /// This is the fastest option - calls go directly to Rust code with no
    /// serialization overhead.
    System(Arc<dyn SystemComponent>),

    /// Another Wasm instance in the same process.
    ///
    /// Calls are bridged through host functions, providing memory isolation
    /// while staying in-process.
    LocalInstance(InstanceHandle),

    /// A remote instance accessed via a Transport.
    ///
    /// Calls are serialized to bytes, sent over the transport, and deserialized
    /// on the remote peer. Full process and network isolation.
    Remote {
        /// The transport to send RPC bytes through.
        transport: Arc<dyn Transport>,
        /// The target identifier on the remote peer.
        target_id: String,
    },
}

/// Fluent builder for configuring and instantiating components.
pub struct InstanceBuilder<'a> {
    rt: &'a Runtime,
    app_id: AppId,
    budget: Budget,
    links: HashMap<String, Linkable>,
}

impl<'a> InstanceBuilder<'a> {
    /// Create a new builder for the given app.
    ///
    /// Starts with default budget and no imports linked.
    pub fn new(rt: &'a Runtime, app_id: AppId) -> Self {
        Self {
            rt,
            app_id,
            budget: Budget::standard(),
            links: HashMap::new(),
        }
    }

    /// Set the resource budget for this instance.
    ///
    /// Controls fuel (instruction count) and memory limits.
    pub fn budget(mut self, budget: Budget) -> Self {
        self.budget = budget;
        self
    }

    /// Link an import to a Linkable target.
    ///
    /// The `name` should match the import name in the component's WIT interface.
    pub fn link(mut self, name: &str, target: Linkable) -> Self {
        self.links.insert(name.to_string(), target);
        self
    }

    /// Helper: Link to a specific Remote Address (Peer + Target ID).
    /// e.g. .link_remote("kv", RemoteAddr { peer: peer_tcp, target_id: "kv-primary" })
    pub async fn link_remote(
        mut self,
        name: &str,
        addr: RemoteAddr
    ) -> Result<Self> {
        let peers = self.rt.inner.peers.lock().await;
        let transport = peers.get(&addr.peer).ok_or_else(|| anyhow!("Peer not found"))?;

        self.links.insert(name.to_string(), Linkable::Remote { 
            transport: transport.clone(),
            target_id: addr.target_id,
        });
        Ok(self)
    }

    /// Link to a local system implementation (Rust code).
    ///
    /// This is a convenience wrapper around `.link()` for System components.
    pub fn link_system(mut self, name: &str, sys: impl SystemComponent) -> Self {
        self.links.insert(name.to_string(), Linkable::System(Arc::new(sys)));
        self
    }

    /// Instantiate the component with the configured links.
    ///
    /// This consumes the builder and returns a running instance.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The app hasn't been registered
    /// - Linking fails (missing imports, type mismatches)
    /// - Instantiation fails (start function traps, etc.)
    pub async fn instantiate(self) -> Result<InstanceHandle> {
        let apps = self.rt.inner.apps.lock().await;
        let component = apps.get(&self.app_id).ok_or_else(|| anyhow!("App not found"))?;

        let mut linker = Linker::<IsorunCtx>::new(&self.rt.inner.engine);
        
        // Add WASI support by default
        wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
        
        let mut ctx_builder = ContextBuilder::new();

        // --- The Wiring Loop ---
        let links = self.links;
        
        for (name, target) in links {
            match target {
                Linkable::System(sys) => {
                    sys.install(&mut linker)?;
                    sys.configure(&mut ctx_builder)?;
                }
                Linkable::LocalInstance(handle) => {
                    crate::linker::link_local_instance(&mut linker, &name, handle).await?;
                }
                Linkable::Remote { transport, target_id } => {
                    crate::linker::link_remote(&mut linker, &name, component, transport, target_id)?;
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
            component: Arc::new(component.clone()),
        })
    }
}
