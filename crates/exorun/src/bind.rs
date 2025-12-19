//! # Dynamic linker for local components and peer components
//!
//! This module prepares the glue required to call
//! WIT instance methods across a network/runtime boundary.
//! It iterates over the static `Ledger`,
//! generates Wasmtime-compatible async host closures,
//! and wires them into the `Linker`.

use wasmtime::component::Linker;
use wasmtime::component::LinkerInstance;
use wasmtime::component::Type;
use wasmtime::component::Val;
use neorpc::CallEncoder;

use crate::context::ExorunCtx;
use crate::local::LocalInstance;
use crate::local::State;
use crate::ledger::Ledger;
use crate::runtime::PeerId;
use crate::peer::PeerInstance;

#[derive(Debug)]
pub enum Error {
    /// The interface requested for linking was not found in the Ledger.
    InterfaceNotFound(String),
    /// Interface export not found in component.
    InterfaceExportNotFound { interface: String },
    /// Function export not found in interface.
    FunctionExportNotFound { interface: String, function: String },
    /// Wasmtime linker error (e.g., duplicate definition, shadow disabled).
    Linker(wasmtime::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InterfaceNotFound(name) => write!(f, "Interface '{}' not found in Ledger", name),
            Self::InterfaceExportNotFound { interface } => { write!(f, "Interface '{}' not found in component exports", interface) }
            Self::FunctionExportNotFound { interface, function } => { write!(f, "Function '{}' not found in interface '{}'", function, interface) }
            Self::Linker(e) => write!(f, "Linker error: {}", e),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// The Binder orchestrates the wiring of imports.
pub struct Binder;

impl Binder {
    /// Links a specific interface instance (e.g., `my:kv/store`) to a remote target.
    ///
    /// This will iterate over all functions defined in the Ledger for this interface
    /// and generate a stub for each one. The actual Client is resolved at call time
    /// from the Runtime via the peer_id, enabling reconnection support.
    pub fn peer_interface(
        linker: &mut Linker<ExorunCtx>,
        ledger: &Ledger,
        interface_name: &str,
        target: PeerInstance,
    ) -> Result<()> {
        let schema = ledger.interfaces.get(interface_name)
            .ok_or_else(|| Error::InterfaceNotFound(interface_name.to_string()))?;

        let mut linker_instance = linker.instance(interface_name)
            .map_err(Error::Linker)?;

        for (method_name, signature) in schema.funcs.iter() {
            Binder::peer_method(
                &mut linker_instance,
                method_name,
                target.peer_id.clone(),
                target.target_id.clone(),
                signature.results.clone(),
            )?;
        }

        Ok(())
    }

    // TODO: we pass result types here but maybe we can
    //       prepare special data for the decoder that
    //       has instructions for how to decode specific types
    //       and we calculate this once instead of tree-walking
    /// Generates the async closure for a specific method within an instance.
    /// The closure resolves the peer_id to a Peer at call time via the Runtime
    /// in ExorunCtx, enabling transparent reconnection.
    fn peer_method(
        linker_instance: &mut LinkerInstance<ExorunCtx>,
        method_name: &str,
        peer_id: PeerId,
        target_id: String,
        result_types: Vec<Type>,
    ) -> Result<()> {
        let method_name_owned = method_name.to_string();

        linker_instance.func_new_async(method_name, move |store, _func_ty, args, results| {
            let peer_id = peer_id.clone();
            let result_types = result_types.clone();
            let target_id = target_id.clone();
            let method_name = method_name_owned.clone();

            // TODO: get rid of map_err by writing helper function
            //       or automatic conversion for given error types
            Box::new(async move {
                // Get runtime from store context and resolve peer_id to peer
                let runtime = store.data().runtime.clone();
                let peer = runtime.get_peer(peer_id)
                    .map_err(|e| wasmtime::Error::msg(e.to_string()))?;

                // prepare the call by incrementing seq and reserving pending
                let (seq, rx) = peer.prepare_call(result_types);

                // encode arguments directly without copying
                let args_bytes = neorpc::encode_vals_to_bytes(args)
                    .map_err(|e| wasmtime::Error::msg(e.to_string()))?;

                // build the payload
                let payload = CallEncoder::new(seq, &target_id, &method_name, &args_bytes)
                    .into_bytes()
                    .map_err(|e| wasmtime::Error::msg(e.to_string()))?;

                // send and await response
                let return_vals = peer.send_and_await(seq, payload, rx)
                    .await
                    .map_err(|e| wasmtime::Error::msg(e.to_string()))?;

                // copy out return vals
                for (i, val) in return_vals.into_iter().enumerate() {
                    results[i] = val;
                }

                Ok(())
            })
        }).map_err(Error::Linker)?;

        Ok(())
    }

    /// Links a specific interface to a local instance.
    ///
    /// This creates direct bindings to another Wasm instance in the same process,
    /// bypassing serialization and using direct Val-to-Val calls.
    pub fn local_interface(
        linker: &mut Linker<ExorunCtx>,
        ledger: &Ledger,
        interface_name: &str,
        target: LocalInstance,
    ) -> Result<()> {
        let schema = ledger.interfaces.get(interface_name)
            .ok_or_else(|| Error::InterfaceNotFound(interface_name.to_string()))?;

        let mut linker_instance = linker.instance(interface_name)
            .map_err(Error::Linker)?;

        // Precompute interface export index
        let inst_idx = target.component
            .get_export_index(None, interface_name)
            .ok_or_else(|| Error::InterfaceExportNotFound {
                interface: interface_name.to_string(),
            })?;

        for (method_name, signature) in schema.funcs.iter() {
            // Precompute function export index
            let func_idx = target.component
                .get_export_index(Some(&inst_idx), method_name)
                .ok_or_else(|| Error::FunctionExportNotFound {
                    interface: interface_name.to_string(),
                    function: method_name.to_string(),
                })?;

            Binder::local_method(
                &mut linker_instance,
                method_name,
                target.clone(),
                func_idx,
                signature.results.len(),
            )?;
        }

        Ok(())
    }

    /// Generates the async closure for a local method call to another instance.
    fn local_method(
        linker_instance: &mut LinkerInstance<ExorunCtx>,
        method_name: &str,
        target: LocalInstance,
        func_idx: wasmtime::component::ComponentExportIndex,
        result_count: usize,
    ) -> Result<()> {
        linker_instance.func_new_async(method_name, move |_store, _func_ty, args, results| {
            let target = target.clone();
            let func_idx = func_idx.clone();
            let args_vec: Vec<Val> = args.to_vec();

            Box::new(async move {
                let mut guard = target.inner.lock().await;
                let State { store, instance } = &mut *guard;

                let func = instance
                    .get_func(&mut *store, &func_idx)
                    .ok_or_else(|| wasmtime::Error::msg("Failed to get function from instance"))?;

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
        }).map_err(Error::Linker)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use neopack::Decoder;
    use neopack::Encoder;
    use neorpc::ReplyOkEncoder;
    use neorpc::RpcFrame;
    use neorpc::encode_vals_to_bytes;
    use wasmtime::component::Component;
    use wasmtime::Engine;
    use wasmtime::Store;
    use tokio::sync::Mutex;

    use super::*;
    use crate::peer::Peer;
    use crate::runtime::Runtime;
    use crate::transport::Transport;

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

        // Create runtime and register peer
        let runtime = Arc::new(Runtime::with_engine(engine.clone()));
        let transport = Box::new(MockTransport::new());
        let peer = Arc::new(Peer::new("test-peer", transport));
        let peer_id = runtime.add_peer(peer);

        let mut linker = Linker::<ExorunCtx>::new(&engine);
        let target = PeerInstance {
            peer_id,
            target_id: "service-1".to_string(),
        };

        Binder::peer_interface(&mut linker, &ledger, "my:service/api", target)
            .expect("Binding failed");

        let ctx = crate::context::ContextBuilder::new().build(Arc::clone(&runtime));
        let mut store = Store::new(&engine, ctx);
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
        let peer_id = crate::runtime::PeerId(1);  // Use a dummy peer ID
        let target = PeerInstance {
            peer_id,
            target_id: "s".into(),
        };

        let err = Binder::peer_interface(&mut linker, &ledger, "missing:interface", target)
            .unwrap_err();

        match err {
            Error::InterfaceNotFound(name) => assert_eq!(name, "missing:interface"),
            _ => panic!("Wrong error type"),
        }
    }
}
