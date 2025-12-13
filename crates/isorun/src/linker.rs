use std::sync::Arc;
use anyhow::{anyhow, Result};
use wasmtime::component::{Linker, Val, Type};
use crate::context::IsorunCtx;
use crate::instance::InstanceHandle;
use crate::traits::Transport;

fn decode_val(decoder: &mut neopack::Decoder, ty: &Type) -> Result<Val> {
    match ty {
        Type::Bool => Ok(Val::Bool(decoder.bool()?)),
        Type::U32 => Ok(Val::U32(decoder.u32()?)),
        Type::S32 => Ok(Val::S32(decoder.s32()?)),
        Type::U64 => Ok(Val::U64(decoder.u64()?)),
        Type::S64 => Ok(Val::S64(decoder.s64()?)),
        Type::String => Ok(Val::String(decoder.str()?.to_string())),
        _ => Err(anyhow!("Unsupported type in generic linker: {:?}", ty)),
    }
}

pub async fn link_local_instance(
    linker: &mut Linker<IsorunCtx>,
    import_name: &str,
    target: InstanceHandle,
) -> Result<()> {
    let exports = crate::introspect::discover_exports(&target.component)?;
    let matching_exports: Vec<_> = exports
        .iter()
        .filter(|e| e.interface.as_deref() == Some(import_name))
        .collect();

    if matching_exports.is_empty() {
        return Err(anyhow!("No exports found for interface '{}'", import_name));
    }

    let mut interface = linker.instance(import_name)?;
    for export in matching_exports {
        let func_name = export.func_name.clone();
        let full_name = export.name.clone();
        let target_clone = target.clone();
        let result_count = export.results.len();

        interface.func_new_async(
            &func_name,
            move |_caller, _func_ty, args: &[Val], results: &mut [Val]| {
                let target = target_clone.clone();
                let full_name = full_name.clone();
                let args = args.to_vec();
                Box::new(async move {
                    let result_vals = target.exec(move |store, instance| {
                        let args = args.clone();
                        let full_name = full_name.clone();
                        Box::pin(async move {
                            let func = instance.get_func(&mut *store, &full_name)
                                .ok_or_else(|| anyhow!("Function not found: {}", full_name))?;
                            let mut storage = vec![Val::Bool(false); result_count];
                            func.call_async(&mut *store, &args, &mut storage).await?;
                            Ok(storage)
                        })
                    }).await?;
                    for (i, val) in result_vals.into_iter().enumerate() {
                        if i < results.len() { results[i] = val; }
                    }
                    Ok(())
                })
            },
        )?;
    }
    Ok(())
}

pub fn link_remote(
    linker: &mut Linker<IsorunCtx>,
    import_name: &str,
    consumer: &wasmtime::component::Component,
    transport: Arc<dyn Transport>,
    target_id: String,
) -> Result<()> {
    let imports = crate::introspect::discover_imports(consumer)?;
    let matching_imports: Vec<_> = imports
        .iter()
        .filter(|i| i.interface.as_deref() == Some(import_name))
        .collect();

    if matching_imports.is_empty() {
        return Err(anyhow!("No imports found for interface '{}'", import_name));
    }

    let mut interface = linker.instance(import_name)?;
    for import in matching_imports {
        let func_name = import.func_name.clone();
        let transport_clone = transport.clone();
        let target_id_clone = target_id.clone();
        let result_types = import.results.clone();

        interface.func_new_async(
            &func_name,
            move |_caller, _func_ty, args: &[Val], results: &mut [Val]| {
                let transport = transport_clone.clone();
                let target_id = target_id_clone.clone();
                let func_name = func_name.clone();
                let result_types = result_types.clone();
                let args = args.to_vec();

                Box::new(async move {
                    let mut encoder = neopack::Encoder::new();
                    let seq = crate::rpc::next_seq();
                    neorpc::CallFrame::encode(&mut encoder, seq, &target_id, &func_name, &args)
                        .map_err(|e| anyhow!("Encode error: {:?}", e))?;
                    let payload = encoder.into_bytes().map_err(|e| anyhow!("Encoder error: {:?}", e))?;

                    let response_bytes = transport.call(&payload).await?;
                    let decoder = neopack::Decoder::new(&response_bytes);
                    let reply = neorpc::ReplyFrame::decode(decoder).map_err(|e| anyhow!("Decode error: {:?}", e))?;

                    match reply.status {
                        Ok(mut results_dec) => {
                            let mut list_iter = results_dec.list().map_err(|e| anyhow!("List error: {:?}", e))?;
                            for (i, ty) in result_types.iter().enumerate() {
                                if let Some(mut item_dec) = list_iter.next() {
                                    if i < results.len() {
                                        results[i] = decode_val(&mut item_dec, ty)?;
                                    }
                                }
                            }
                            Ok(())
                        }
                        Err(reason) => Err(anyhow!("Remote call failed: {:?}", reason)),
                    }
                })
            },
        )?;
    }
    Ok(())
}
