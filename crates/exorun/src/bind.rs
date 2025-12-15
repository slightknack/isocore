//! # Dynamic Linker & Closure Factory
//!
//! This module mechanically instantiates the "Airlock" for remote functions.
//! It iterates over the static `Ledger`, generates Wasmtime-compatible async host
//! closures, and wires them into the `Linker`.
//!
//! ## Architecture
//!
//! - **Binder**: The entry point for linking.
//! - **Closure Factory**: A higher-order mechanism that captures the `Transport`
//!   and `target_id`, returning a `Fn` that Wasmtime can execute.
//! - **Sequence Generation**: Uses per-instance sequence counter from `ExorunCtx` for RPC correlation.

use std::sync::Arc;

use wasmtime::component::Linker;
use wasmtime::component::LinkerInstance;
use wasmtime::component::Type;
use wasmtime::component::Val;

use crate::client::Client;
use crate::context::ExorunCtx;
use crate::instance::InstanceHandle;
use crate::instance::State;
use crate::ledger::Ledger;
use crate::transport::Transport;

#[derive(Debug)]
pub enum Error {
    /// The interface requested for linking was not found in the Ledger.
    InterfaceNotFound(String),
    /// Wasmtime linker error (e.g., duplicate definition, shadow disabled).
    Wasmtime(wasmtime::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InterfaceNotFound(name) => write!(f, "Interface '{}' not found in Ledger", name),
            Self::Wasmtime(e) => write!(f, "Wasmtime linker error: {}", e),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// A handle to the resources needed to bind a remote target.
#[derive(Clone)]
pub struct RemoteTarget {
    pub transport: Arc<dyn Transport>,
    pub target_id: String,
}

/// The Binder orchestrates the wiring of imports.
pub struct Binder;

impl Binder {
    /// Links a specific interface instance (e.g., `my:kv/store`) to a remote target.
    ///
    /// This will iterate over all functions defined in the Ledger for this interface
    /// and generate a stub for each one.
    pub fn link_remote_interface(
        linker: &mut Linker<ExorunCtx>,
        ledger: &Ledger,
        interface_name: &str,
        target: RemoteTarget,
    ) -> Result<()> {
        let schema = ledger.interfaces.get(interface_name)
            .ok_or_else(|| Error::InterfaceNotFound(interface_name.to_string()))?;

        let mut linker_instance = linker.instance(interface_name)
            .map_err(Error::Wasmtime)?;

        // Create the client once for this target
        let client = Client::new(target.transport);

        for (method_name, signature) in schema.funcs.iter() {
            bind_method(
                &mut linker_instance,
                method_name,
                target.target_id.clone(),
                client.clone(),
                signature.results.clone(),
            )?;
        }

        Ok(())
    }

    /// Links a specific interface to a local instance.
    ///
    /// This creates direct bindings to another Wasm instance in the same process,
    /// bypassing serialization and using direct Val-to-Val calls.
    pub fn link_local_interface(
        linker: &mut Linker<ExorunCtx>,
        ledger: &Ledger,
        interface_name: &str,
        target: InstanceHandle,
    ) -> Result<()> {
        let schema = ledger.interfaces.get(interface_name)
            .ok_or_else(|| Error::InterfaceNotFound(interface_name.to_string()))?;

        let mut linker_instance = linker.instance(interface_name)
            .map_err(Error::Wasmtime)?;

        for (method_name, signature) in schema.funcs.iter() {
            bind_local_method(
                &mut linker_instance,
                method_name,
                target.clone(),
                signature.params.len(),
                signature.results.len(),
            )?;
        }

        Ok(())
    }
}

// TODO: we pass result types here but maybe we can
//       prepare special data for the decoder that
//       has instructions for how to decode specific types
//       and we calculate this once instead of tree-walking
/// Generates the async closure for a specific method within an instance.
fn bind_method(
    linker_instance: &mut LinkerInstance<ExorunCtx>,
    method_name: &str,
    target_id: String,
    client: Client,
    result_types: Vec<Type>,
) -> Result<()> {
    let method_name_clone = method_name.to_string();

    linker_instance.func_new_async(method_name, move |_store, _func_ty, args, results| {
        use neopack::Encoder;
        use neorpc::CallEncoder;
        
        let client = client.clone();
        let result_types = result_types.clone();
        let target_id = target_id.clone();
        let method_name = method_name_clone.clone();

        Box::new(async move {
            // Prepare the call (increments seq and inserts pending)
            let (seq, rx) = client.prepare_call(result_types);
            
            // Encode arguments directly without copying
            let args_bytes = neorpc::encode_vals_to_bytes(args)
                .map_err(|e| wasmtime::Error::msg(e.to_string()))?;
            
            // Build the payload using CallEncoder
            let mut enc = Encoder::new();
            CallEncoder::new(seq, &target_id, &method_name, &args_bytes)
                .encode(&mut enc)
                .map_err(|e| wasmtime::Error::msg(e.to_string()))?;
            let payload = enc.into_bytes()
                .map_err(|e| wasmtime::Error::msg(e.to_string()))?;
            
            // Send and await response
            let return_vals = client.send_and_await(seq, payload, rx)
                .await
                .map_err(|e| wasmtime::Error::msg(e.to_string()))?;

            if return_vals.len() != results.len() {
                return Err(wasmtime::Error::msg(format!(
                    "Result count mismatch: expected {}, got {}",
                    results.len(),
                    return_vals.len()
                )));
            }

            for (i, val) in return_vals.into_iter().enumerate() {
                results[i] = val;
            }

            Ok(())
        })
    }).map_err(Error::Wasmtime)?;

    Ok(())
}

/// Generates the async closure for a local method call to another instance.
fn bind_local_method(
    linker_instance: &mut LinkerInstance<ExorunCtx>,
    method_name: &str,
    target: InstanceHandle,
    _param_count: usize,
    result_count: usize,
) -> Result<()> {
    let method_name_owned = method_name.to_string();

    linker_instance.func_new_async(method_name, move |_store, _func_ty, args, results| {
        let target = target.clone();
        let method_name = method_name_owned.clone();
        let args_vec: Vec<Val> = args.to_vec();

        Box::new(async move {
            // Lock the target instance and call the function
            let mut guard = target.inner.lock().await;
            let State { store, instance } = &mut *guard;
            
            let func = instance
                .get_func(&mut *store, &method_name)
                .ok_or_else(|| wasmtime::Error::msg(format!("Method '{}' not found", method_name)))?;

            let mut call_results = vec![Val::Bool(false); result_count];
            func.call_async(&mut *store, &args_vec, &mut call_results)
                .await?;

            if call_results.len() != results.len() {
                return Err(wasmtime::Error::msg(format!(
                    "Result count mismatch: expected {}, got {}",
                    results.len(),
                    call_results.len()
                )));
            }

            for (i, val) in call_results.into_iter().enumerate() {
                results[i] = val;
            }

            Ok(())
        })
    }).map_err(Error::Wasmtime)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use neopack::Decoder;
    use neopack::Encoder;
    use neorpc::ReplyOkEncoder;
    use neorpc::RpcFrame;
    use neorpc::encode_vals_to_bytes;
    use wasmtime::component::Component;
    use wasmtime::Engine;
    use wasmtime::Store;

    use tokio::sync::Mutex;

    struct MockTransport {
        pending: Arc<Mutex<Option<Vec<u8>>>>,
    }

    impl MockTransport {
        fn new() -> Self {
            Self {
                pending: Arc::new(Mutex::new(None)),
            }
        }
    }

    #[async_trait::async_trait]
    impl Transport for MockTransport {
        async fn send(&self, payload: &[u8]) -> crate::transport::Result<()> {
            let mut dec = Decoder::new(payload);
            let frame = RpcFrame::decode(&mut dec).expect("Mock received invalid frame");

            let seq = match frame {
                RpcFrame::Call(c) => c.seq,
                _ => panic!("Mock received non-call frame"),
            };

            let mut enc = Encoder::new();
            let empty_bytes = encode_vals_to_bytes(&[]).unwrap();
            ReplyOkEncoder::new(seq, &empty_bytes).encode(&mut enc).unwrap();
            let response = enc.into_bytes().unwrap();
            *self.pending.lock().await = Some(response);
            Ok(())
        }

        async fn recv(&self) -> crate::transport::Result<Option<Vec<u8>>> {
            Ok(self.pending.lock().await.take())
        }
    }

    fn compile_component(engine: &Engine, wat: &str) -> Component {
        Component::new(engine, wat).unwrap()
    }

    #[tokio::test]
    async fn test_bind_remote_interface_success() {
        let engine = Engine::new(&wasmtime::Config::new().async_support(true)).unwrap();

        let wat = r#"
            (component
                (import "my:service/api" (instance $api
                    (export "ping" (func))
                ))
                (core module $m
                    (import "my:service/api" "ping" (func $ping))
                    (func (export "run")
                        call $ping
                    )
                )
                (core func $ping_lower (canon lower (func $api "ping")))
                (core instance $i (instantiate $m (with "my:service/api" (instance (export "ping" (func $ping_lower))))))
                (func (export "run") (canon lift (core func $i "run")))
            )
        "#;

        let component = compile_component(&engine, wat);
        let ledger = Ledger::from_component(&component).unwrap();

        let mut linker = Linker::<ExorunCtx>::new(&engine);
        let transport = Arc::new(MockTransport::new());
        let target = RemoteTarget {
            transport,
            target_id: "service-1".to_string(),
        };

        Binder::link_remote_interface(&mut linker, &ledger, "my:service/api", target)
            .expect("Binding failed");

        let mut store = Store::new(&engine, ExorunCtx::new());
        let instance = linker.instantiate_async(&mut store, &component).await
            .expect("Instantiation failed");

        let run = instance.get_typed_func::<(), ()>(&mut store, "run")
            .expect("Get func failed");
        run.call_async(&mut store, ()).await.expect("Execution failed");
    }

    #[tokio::test]
    async fn test_bind_missing_interface_error() {
        let engine = Engine::default();
        let wat = r#"(component)"#;
        let component = compile_component(&engine, wat);
        let ledger = Ledger::from_component(&component).unwrap();

        let mut linker = Linker::<ExorunCtx>::new(&engine);
        let target = RemoteTarget {
            transport: Arc::new(MockTransport::new()),
            target_id: "s".into(),
        };

        let err = Binder::link_remote_interface(&mut linker, &ledger, "missing:interface", target)
            .unwrap_err();

        match err {
            Error::InterfaceNotFound(name) => assert_eq!(name, "missing:interface"),
            _ => panic!("Wrong error type"),
        }
    }
}
