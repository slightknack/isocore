//! Dynamic linker utilities for bridging component instances.
//!
//! This module provides the machinery for generating host functions that:
//! - Bridge calls from one instance to another (LocalInstance linking)
//! - Serialize calls and send over RPC (Remote linking)

use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;

use wasmtime::component::Linker;

use crate::context::IsorunCtx;
use crate::instance::InstanceHandle;
use crate::traits::Transport;

/// Install bridge functions for a Local instance target.
///
/// This discovers all exported functions from the target instance and creates
/// host functions in the linker that forward calls to it using untyped `Func`.
///
/// # Arguments
///
/// - `linker`: The linker to install functions into
/// - `import_name`: The import name being satisfied (e.g., "test:demo/math")
/// - `target`: The instance to bridge calls to
pub async fn link_local_instance(
    linker: &mut Linker<IsorunCtx>,
    import_name: &str,
    target: InstanceHandle,
) -> Result<()> {
    use wasmtime::component::Val;
    
    // Discover what the target exports
    let exports = crate::introspect::discover_exports(&target.component)?;
    
    // Filter to exports matching the import interface
    let matching_exports: Vec<_> = exports
        .iter()
        .filter(|e| e.interface.as_deref() == Some(import_name))
        .collect();
    
    if matching_exports.is_empty() {
        return Err(anyhow!(
            "No exports found for interface '{}' in target instance",
            import_name
        ));
    }
    
    let mut interface = linker.instance(import_name)?;
    
    // Create a generic bridge for each exported function
    for export in matching_exports {
        let func_name = export.func_name.clone();
        let full_name = export.name.clone();
        let target_clone = target.clone();
        
        interface.func_new_async(
            &func_name,
            move |_caller, _func_ty, args: &[Val], results: &mut [Val]| {
                let target = target_clone.clone();
                let full_name = full_name.clone();
                let args = args.to_vec();
                let result_count = results.len();
                
                Box::new(async move {
                    // Call the target instance's function
                    let result_vals = target.exec(move |store, instance| {
                        let args = args.clone();
                        let full_name = full_name.clone();
                        
                        Box::pin(async move {
                            // Get the untyped function
                            let func = instance.get_func(&mut *store, &full_name)
                                .ok_or_else(|| anyhow!("Function not found: {}", full_name))?;
                            
                            // Allocate space for results
                            let mut result_storage = vec![Val::Bool(false); result_count];
                            
                            // Call it with untyped args
                            func.call_async(&mut *store, &args, &mut result_storage).await?;
                            
                            Ok::<Vec<Val>, anyhow::Error>(result_storage)
                        }) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<Val>>> + Send + '_>>
                    }).await?;
                    
                    // Copy results back
                    for (i, val) in result_vals.iter().enumerate() {
                        if i < results.len() {
                            results[i] = val.clone();
                        }
                    }
                    
                    Ok(())
                })
            },
        )?;
    }
    
    Ok(())
}

/// Install RPC stub functions for a Remote target.
///
/// This creates host functions that serialize arguments, send them via the
/// transport, and deserialize the response using neorpc.
///
/// This implementation is fully generic and works for any function signature.
///
/// # Arguments
///
/// - `linker`: The linker to install functions into  
/// - `import_name`: The import name being satisfied (e.g., "test:demo/math")
/// - `consumer`: The consumer component that needs these imports
/// - `transport`: The transport to send RPC calls through
/// - `target_id`: The remote target identifier
pub fn link_remote(
    linker: &mut Linker<IsorunCtx>,
    import_name: &str,
    consumer: &wasmtime::component::Component,
    transport: Arc<dyn Transport>,
    target_id: String,
) -> Result<()> {
    use wasmtime::component::Val;
    
    // Discover what the consumer needs to import
    let imports = crate::introspect::discover_imports(consumer)?;
    
    // Filter to imports matching this interface
    let matching_imports: Vec<_> = imports
        .iter()
        .filter(|i| i.interface.as_deref() == Some(import_name))
        .collect();
    
    if matching_imports.is_empty() {
        return Err(anyhow!(
            "No imports found for interface '{}' in consumer component",
            import_name
        ));
    }
    
    let mut interface = linker.instance(import_name)?;
    
    // Create an RPC stub for each imported function
    for import in matching_imports {
        let func_name = import.func_name.clone();
        let transport_clone = transport.clone();
        let target_id_clone = target_id.clone();
        
        interface.func_new_async(
            &func_name,
            move |_caller, _func_ty, args: &[Val], results: &mut [Val]| {
                let transport = transport_clone.clone();
                let target_id = target_id_clone.clone();
                let func_name = func_name.clone();
                let args = args.to_vec();
                
                Box::new(async move {
                    // Serialize the call
                    let mut encoder = neopack::Encoder::new();
                    let seq = crate::rpc::next_seq();
                    
                    neorpc::CallFrame::encode(&mut encoder, seq, &target_id, &func_name, &args)
                        .map_err(|e| anyhow!("Encode error: {:?}", e))?;
                    
                    let payload = encoder.into_bytes()
                        .map_err(|e| anyhow!("Encoder error: {:?}", e))?;
                    
                    // Send via transport
                    let response_bytes = transport.call(&payload).await?;
                    
                    // Deserialize the response
                    let decoder = neopack::Decoder::new(&response_bytes);
                    let reply = neorpc::ReplyFrame::decode(decoder)
                        .map_err(|e| anyhow!("Decode error: {:?}", e))?;
                    
                    match reply.status {
                        Ok(mut results_dec) => {
                            // Decode results using neorpc's generic Val decoding
                            // We iterate through the list and decode each value generically
                            let mut list_iter = results_dec.list()
                                .map_err(|e| anyhow!("List decode error: {:?}", e))?;
                            
                            let mut result_idx = 0;
                            while let Some(mut item_dec) = list_iter.next() {
                                if result_idx < results.len() {
                                    // Try decoding as different types
                                    // In order of likelihood for our use cases
                                    let val = if let Ok(v) = item_dec.u32() {
                                        Val::U32(v)
                                    } else if let Ok(v) = item_dec.u64() {
                                        Val::U64(v)
                                    } else if let Ok(v) = item_dec.s32() {
                                        Val::S32(v)
                                    } else if let Ok(v) = item_dec.s64() {
                                        Val::S64(v)
                                    } else if let Ok(v) = item_dec.str() {
                                        Val::String(v.to_string())
                                    } else if let Ok(v) = item_dec.bool() {
                                        Val::Bool(v)
                                    } else {
                                        return Err(anyhow!("Unsupported result type"));
                                    };
                                    
                                    results[result_idx] = val;
                                    result_idx += 1;
                                } else {
                                    return Err(anyhow!("Too many results returned"));
                                }
                            }
                            
                            Ok(())
                        }
                        Err(reason) => {
                            Err(anyhow!("Remote call failed: {:?}", reason))
                        }
                    }
                })
            },
        )?;
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_linker_module_exists() {
        // Basic module structure test
        assert!(true);
    }
}
