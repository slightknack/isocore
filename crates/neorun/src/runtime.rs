use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use wasmtime::{component::Component, Engine};

use crate::handles::{AppId, PeerId};
use crate::instance::InstanceHandle;
use crate::traits::Transport;
use neorpc::{self, ReplyFrame, RpcFrame};

/// The central runtime registry.
#[derive(Clone)]
pub struct Runtime {
    pub(crate) inner: Arc<RuntimeInner>,
}

pub(crate) struct RuntimeInner {
    pub(crate) engine: Engine,
    pub(crate) apps: Mutex<HashMap<AppId, Component>>,
    pub(crate) peers: Mutex<HashMap<PeerId, Arc<dyn Transport>>>,
    pub(crate) instances: Mutex<HashMap<String, InstanceHandle>>,
    next_id: std::sync::atomic::AtomicU64,
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

    pub async fn register_app(&self, _name: &str, bytes: &[u8]) -> Result<AppId> {
        let component = Component::new(&self.inner.engine, bytes)?;
        let id = AppId(
            self.inner
                .next_id
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        );
        self.inner.apps.lock().await.insert(id, component);
        Ok(id)
    }

    pub async fn add_peer(&self, transport: impl Transport) -> Result<PeerId> {
        let id = PeerId(
            self.inner
                .next_id
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        );
        self.inner.peers.lock().await.insert(id, Arc::new(transport));
        Ok(id)
    }

    pub async fn register_instance(&self, target_id: String, handle: InstanceHandle) -> Result<()> {
        self.inner.instances.lock().await.insert(target_id, handle);
        Ok(())
    }

    pub async fn handle_rpc(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let seq = match neorpc::decode_seq(payload) {
            Ok(s) => s,
            Err(_e) => {
                let mut enc = neopack::Encoder::new();
                let reason = neorpc::FailureReason::ProtocolViolation("Invalid frame".into());
                ReplyFrame::encode_failure(&mut enc, 0, &reason)?;
                return Ok(enc.into_bytes()?);
            }
        };

        let result: Result<Vec<wasmtime::component::Val>> = async {
            let mut dec = neopack::Decoder::new(payload);
            let frame = RpcFrame::decode(&mut dec)?;
            let call = match frame {
                RpcFrame::Call(call) => call,
                _ => return Err(anyhow!("Received non-call frame")),
            };

            let target_id = call.target.to_string();
            let method = call.method.to_string();

            let instance_handle = self
                .inner
                .instances
                .lock()
                .await
                .get(&target_id)
                .cloned()
                .ok_or_else(|| anyhow!("Instance '{}' not found", target_id))?;

            let func = instance_handle.get_export_func(&method)
                .ok_or_else(|| anyhow!("Method '{}' not found on target '{}'", method, target_id))?;

            let param_types: Vec<_> = func.params().map(|(_, ty)| ty).collect();
            let result_types: Vec<_> = func.results().collect();

            let args = neorpc::decode_vals(call.args_decoder, &param_types)?;

            let results_vec = instance_handle.exec(move |mut store, instance| Box::pin(async move {
                let func = instance.get_func(&mut store, &method).unwrap();
                let mut call_results = vec![wasmtime::component::Val::U32(0); result_types.len()];
                func.call_async(&mut store, &args, &mut call_results).await?;
                func.post_return_async(&mut store).await?;
                Ok(call_results)
            })).await?;

            Ok(results_vec)
        }
        .await;

        let mut enc = neopack::Encoder::new();
        match result {
            Ok(results) => {
                ReplyFrame::encode_success(&mut enc, seq, &results)?;
            }
            Err(e) => {
                eprintln!("RPC handler error: {}", e);
                let reason = neorpc::FailureReason::AppTrapped;
                ReplyFrame::encode_failure(&mut enc, seq, &reason)?;
            }
        }
        Ok(enc.into_bytes()?)
    }
}