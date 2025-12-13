
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;
use wasmtime::{AsContextMut, Store, StoreContextMut};
use wasmtime::component::types::{ComponentItem, ComponentFunc};

use crate::context::IsorunCtx;

#[derive(Clone)]
pub struct InstanceHandle {
    pub(crate) store: Arc<Mutex<Store<IsorunCtx>>>,
    pub(crate) instance: wasmtime::component::Instance,
    pub(crate) component: Arc<wasmtime::component::Component>,
}

impl InstanceHandle {
    pub async fn exec<F, R>(&self, f: F) -> Result<R>
    where
        F: for<'a> FnOnce(
                &'a mut StoreContextMut<'a, IsorunCtx>,
                &'a wasmtime::component::Instance,
            ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<R>> + Send + 'a>>
            + Send,
        R: Send,
    {
        let mut lock = self.store.lock().await;
        let ctx = &mut lock.as_context_mut();
        f(ctx, &self.instance).await
    }

    /// Finds an exported function's type signature by name.
    pub fn get_export_func(&self, name: &str) -> Option<ComponentFunc> {
        let engine = self.component.engine();
        for (export_name, export_ty) in self.component.component_type().exports(engine) {
            if export_name == name {
                if let ComponentItem::ComponentFunc(func_ty) = export_ty {
                    return Some(func_ty);
                }
            }
        }
        None
    }
}
