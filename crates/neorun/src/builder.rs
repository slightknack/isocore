
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::Arc;
use wasmtime::component::Linker;
use wasmtime::Store;

use crate::context::{ContextBuilder, IsorunCtx};
use crate::handles::{AppId, RemoteAddr};
use crate::instance::InstanceHandle;
use crate::runtime::Runtime;
use crate::traits::Transport;

#[derive(Clone)]
pub enum Linkable {
    Remote {
        transport: Arc<dyn Transport>,
        target_id: String,
    },
}

pub struct InstanceBuilder<'a> {
    rt: &'a Runtime,
    app_id: AppId,
    links: HashMap<String, Linkable>,
}

impl<'a> InstanceBuilder<'a> {
    pub fn new(rt: &'a Runtime, app_id: AppId) -> Self {
        Self {
            rt,
            app_id,
            links: HashMap::new(),
        }
    }

    pub async fn link_remote(mut self, name: &str, addr: RemoteAddr) -> Result<Self> {
        let peers = self.rt.inner.peers.lock().await;
        let transport = peers
            .get(&addr.peer)
            .ok_or_else(|| anyhow!("Peer not found"))?
            .clone();

        self.links.insert(
            name.to_string(),
            Linkable::Remote {
                transport,
                target_id: addr.target_id,
            },
        );
        Ok(self)
    }

    pub async fn instantiate(self) -> Result<InstanceHandle> {
        let apps = self.rt.inner.apps.lock().await;
        let component = apps.get(&self.app_id).ok_or_else(|| anyhow!("App not found"))?;

        let mut linker = Linker::<IsorunCtx>::new(&self.rt.inner.engine);
        let ctx_builder = ContextBuilder::new();

        for (name, target) in self.links {
            match target {
                Linkable::Remote {
                    transport,
                    target_id,
                } => {
                    crate::linker::link_remote(
                        &mut linker,
                        &name,
                        component,
                        transport,
                        target_id,
                    )?;
                }
            }
        }

        let ctx = IsorunCtx::new(ctx_builder);
        let mut store = Store::new(&self.rt.inner.engine, ctx);
        let instance = linker.instantiate_async(&mut store, component).await?;

        Ok(InstanceHandle {
            store: Arc::new(tokio::sync::Mutex::new(store)),
            instance,
            component: Arc::new(component.clone()),
        })
    }
}
