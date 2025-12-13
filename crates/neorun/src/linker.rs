
use anyhow::{anyhow, Result};
use std::sync::Arc;
use wasmtime::component::{Component, Linker};
use wasmtime::component::types::{ComponentFunc, ComponentInstance, ComponentItem};
use crate::context::IsorunCtx;
use crate::traits::Transport;
use neorpc::{self, CallFrame, RpcFrame};

pub fn link_remote(
    linker: &mut Linker<IsorunCtx>,
    import_name: &str,
    component: &Component,
    transport: Arc<dyn Transport>,
    target_id: String,
) -> Result<()> {
    let engine = linker.engine().clone();

    let (_, import_ty) = component
        .component_type()
        .imports(&engine)
        .find(|(name, _)| *name == import_name)
        .ok_or_else(|| anyhow!("Import '{}' not found in component", import_name))?;

    let instance_ty: ComponentInstance = match import_ty {
        ComponentItem::ComponentInstance(ty) => ty,
        _ => return Err(anyhow!("Import '{}' is not an instance interface", import_name)),
    };

    let mut instance_linker = linker.instance(import_name)?;

    for (func_name, func_ty) in instance_ty.exports(&engine) {
        if let ComponentItem::ComponentFunc(_) = func_ty {
            let proxy = create_rpc_proxy(
                transport.clone(),
                target_id.clone(),
                func_name.to_string(),
            );
            instance_linker.func_new_async(&func_name, proxy)?;
        }
    }

    Ok(())
}

fn create_rpc_proxy(
    transport: Arc<dyn Transport>,
    target_id: String,
    method: String,
) -> impl Fn(
    wasmtime::StoreContextMut<'_, IsorunCtx>,
    ComponentFunc,
    &[wasmtime::component::Val],
    &mut [wasmtime::component::Val],
) -> Box<dyn std::future::Future<Output = Result<()>> + Send>
       + Send
       + Sync
       + 'static {
    move |_store, func_ty, args, results| {
        let transport = transport.clone();
        let target_id = target_id.clone();
        let method = method.clone();
        let return_types: Vec<_> = func_ty.results().collect();
        let args_vec = args.to_vec();

        Box::new(async move {
            let future = async {
                let mut enc = neopack::Encoder::new();
                CallFrame::encode(&mut enc, 1, &target_id, &method, &args_vec)?;
                let call_bytes = enc.into_bytes()?;

                let reply_bytes = transport.call(&call_bytes).await?;

                let mut dec = neopack::Decoder::new(&reply_bytes);
                let reply_frame = RpcFrame::decode(&mut dec)?;

                match reply_frame {
                    RpcFrame::Reply(reply) => match reply.status {
                        Ok(result_decoder) => {
                            neorpc::decode_vals(result_decoder, &return_types).map_err(anyhow::Error::from)
                        }
                        Err(reason) => Err(anyhow!("Remote call failed: {:?}", reason)),
                    },
                    _ => Err(anyhow!("Invalid RPC reply frame received")),
                }
            };

            match future.await {
                Ok(result_vals) => {
                    for (i, val) in result_vals.into_iter().enumerate() {
                        results[i] = val;
                    }
                    Ok(())
                }
                Err(e) => Err(e),
            }
        })
    }
}
