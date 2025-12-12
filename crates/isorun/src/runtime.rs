//! The main Runtime registry for apps, peers, and WIT definitions

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;
use wasmtime::component::Component;
use wasmtime::Engine;

use crate::handles::AppId;
use crate::handles::PeerId;
use crate::instance::InstanceHandle;
use crate::rpc::decode_math_add_args;
use crate::rpc::encode_math_add_result;
use crate::rpc::RpcCall;
use crate::rpc::RpcResponse;
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
    // Registry of live instances by their remote identifiers
    pub(crate) instances: Mutex<HashMap<String, InstanceHandle>>,
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
                instances: Mutex::new(HashMap::new()),
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

    /// Register a live instance with a remote identifier.
    /// This allows the instance to receive incoming RPC calls.
    pub async fn register_instance(&self, remote_id: String, handle: InstanceHandle) -> Result<()> {
        self.inner.instances.lock().await.insert(remote_id, handle);
        Ok(())
    }

    /// Unregister an instance by its remote identifier.
    pub async fn unregister_instance(&self, remote_id: &str) -> Result<()> {
        self.inner.instances.lock().await.remove(remote_id);
        Ok(())
    }

    /// Handle an incoming binary payload from a Transport.
    ///
    /// 1. Payload header contains Target RemoteID.
    /// 2. Look up local instance associated with that RemoteID.
    /// 3. Exec.
    pub async fn handle_incoming_rpc(&self, payload: &[u8]) -> Result<Vec<u8>> {
        // Deserialize the RPC call
        let call = RpcCall::from_bytes(payload)?;
        
        // Look up the instance
        let instances = self.inner.instances.lock().await;
        let instance = instances.get(&call.remote_instance)
            .ok_or_else(|| anyhow::anyhow!("Instance not found: {}", call.remote_instance))?
            .clone();
        drop(instances);
        
        // Execute the function based on interface and function name
        // For now, we only handle the math/add function
        if call.interface == "test:demo/math" && call.function == "add" {
            // Decode arguments
            let (a, b) = decode_math_add_args(&call.args)?;
            
            // Call the function
            let mut lock = instance.store.lock().await;
            let math = instance.instance.get_typed_func::<(u32, u32), (u32,)>(&mut *lock, "test:demo#math")?;
            let (result,) = math.call_async(&mut *lock, (a, b)).await?;
            drop(lock);
            
            // Encode the result
            let result_bytes = encode_math_add_result(result)?;
            
            // Create and serialize the response
            let response = RpcResponse::new(result_bytes);
            response.to_bytes()
        } else {
            anyhow::bail!("Unsupported function: {}::{}", call.interface, call.function)
        }
    }
}
