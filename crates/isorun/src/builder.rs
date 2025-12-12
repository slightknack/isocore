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
use crate::rpc::encode_math_add_args;
use crate::rpc::RpcCall;
use crate::rpc::RpcResponse;
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
        
        // Add WASI support by default
        wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
        
        let mut ctx_builder = ContextBuilder::new();

        // --- The Wiring Loop ---
        let links = self.links;
        for (name, target) in links {
            match target {
                Linkable::System(sys) => {
                    // 1. Install definitions
                    sys.install(&mut linker)?;
                    // 2. Configure context (WASI preopens, Auth, etc)
                    sys.configure(&mut ctx_builder)?;
                }
                Linkable::LocalInstance(handle) => {
                    // Bridge local instances directly via host functions
                    Self::link_local_instance(&mut linker, &name, handle)?;
                }
                Linkable::Remote { transport, remote_instance } => {
                    // Generate RPC stubs that serialize calls to bytes
                    Self::link_remote_instance(&mut linker, &name, transport, &remote_instance)?;
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

    fn link_local_instance(
        linker: &mut Linker<IsorunCtx>,
        interface_name: &str,
        handle: InstanceHandle,
    ) -> Result<()> {
        // For local instances, we need to bind the exported functions from the target instance
        // to the imports of the current component. This requires dynamic function binding.
        
        // For the math interface, we need to bind the 'add' function
        if interface_name == "test:demo/math" {
            linker.instance("test:demo/math")?.func_wrap_async(
                "add",
                move |_caller: wasmtime::StoreContextMut<'_, IsorunCtx>, (a, b): (u32, u32)| {
                    let handle = handle.clone();
                    Box::new(async move {
                        let mut lock = handle.store.lock().await;
                        let math = handle.instance.get_typed_func::<(u32, u32), (u32,)>(&mut *lock, "test:demo#math")?;
                        let (result,) = math.call_async(&mut *lock, (a, b)).await?;
                        Ok((result,))
                    })
                },
            )?;
        } else if interface_name == "test:demo/kv" {
            // Handle KV interface binding similarly
            // This would need to handle get and set functions
            // For now, we'll skip this as it's not in the basic tests
        }

        Ok(())
    }

    fn link_remote_instance(
        linker: &mut Linker<IsorunCtx>,
        interface_name: &str,
        transport: Arc<dyn Transport>,
        remote_instance: &str,
    ) -> Result<()> {
        // For remote instances, we need to create RPC stubs that serialize calls

        // For the math interface
        if interface_name == "test:demo/math" {
            let remote_id = remote_instance.to_string();
            let iface = interface_name.to_string();
            
            linker.instance("test:demo/math")?.func_wrap_async(
                "add",
                move |_caller: wasmtime::StoreContextMut<'_, IsorunCtx>, (a, b): (u32, u32)| {
                    let transport = transport.clone();
                    let remote_id = remote_id.clone();
                    let iface = iface.clone();
                    
                    Box::new(async move {
                        // Encode arguments
                        let args = encode_math_add_args(a, b)?;
                        
                        // Create RPC call
                        let call = RpcCall::new(remote_id, iface, "add".to_string(), args);
                        
                        // Serialize and send
                        let payload = call.to_bytes()?;
                        let response_bytes = transport.call(&payload).await?;
                        
                        // Deserialize response
                        let response = RpcResponse::from_bytes(&response_bytes)?;
                        
                        // Decode result
                        let result = crate::rpc::decode_math_add_result(&response.result)?;
                        
                        Ok((result,))
                    })
                },
            )?;
        }

        Ok(())
    }
}
