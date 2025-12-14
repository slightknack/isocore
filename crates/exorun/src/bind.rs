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

use neopack::Encoder;
use neorpc::CallEncoder;
use wasmtime::component::Linker;

use crate::context::ExorunCtx;
use crate::ledger::FunctionSignature;
use crate::ledger::Ledger;
use crate::proxy::Proxy;
use crate::proxy::ProxyError;
use crate::transport::Transport;

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
    ) -> Result<(), Error> {
        let schema = ledger.interfaces.get(interface_name)
            .ok_or_else(|| Error::InterfaceNotFound(interface_name.to_string()))?;

        // We must enter the instance namespace in the linker
        let mut instance_linker = linker.instance(interface_name)
            .map_err(Error::Wasmtime)?;

        for (method_name, signature) in &schema.funcs {
            bind_method(
                &mut instance_linker,
                method_name,
                target.clone(),
                signature.clone()
            )?;
        }

        Ok(())
    }

    /// Links a root-level function (e.g., `log`) to a remote target.
    pub fn link_remote_root(
        linker: &mut Linker<ExorunCtx>,
        ledger: &Ledger,
        func_name: &str,
        target: RemoteTarget,
    ) -> Result<(), Error> {
        let signature = ledger.root_funcs.get(func_name)
            .ok_or_else(|| Error::FunctionNotFound(func_name.to_string()))?;

        let _root = linker.root();
        // Wasmtime's root linker instance is a bit special, usually accessed via root()
        // Here we can just use the implementation logic of bind_method but adapted
        // for the root scope. However, `LinkerInstance` is not easily generic over root vs nested.
        // We replicate the logic for the root.

        let transport = target.transport.clone();
        let target_id = target.target_id.clone();
        let method = func_name.to_string();
        let sig = signature.clone();

        linker.root().func_new_async(func_name, move |store, _func_ty, args, results| {
            let seq = store.data().next_seq();
            let mut enc = Encoder::new();
            CallEncoder::new(seq, &target_id, &method, args).encode(&mut enc).unwrap();
            let payload = enc.into_bytes().unwrap();

            let transport = transport.clone();
            let sig = sig.clone();

            Box::new(async move {
                let reply_vals = Proxy::invoke(&payload, &transport, &sig).await
                    .map_err(map_proxy_error)?;

                if reply_vals.len() != results.len() {
                    return Err(wasmtime::Error::msg(format!(
                        "Ledger mismatch: expected {} results, got {}",
                        results.len(),
                        reply_vals.len()
                    )));
                }

                for (i, val) in reply_vals.into_iter().enumerate() {
                    results[i] = val;
                }

                Ok(())
            })
        }).map_err(Error::Wasmtime)?;

        Ok(())
    }
}

/// Helper to generate the closure for a specific method within an instance.
fn bind_method(
    instance_linker: &mut wasmtime::component::LinkerInstance<ExorunCtx>,
    method_name: &str,
    target: RemoteTarget,
    signature: FunctionSignature,
) -> Result<(), Error> {
    let transport = target.transport;
    let target_id = target.target_id;
    let method = method_name.to_string();

    instance_linker.func_new_async(method_name, move |store, _func_ty, args, results| {
        let seq = store.data().next_seq();
        let mut enc = Encoder::new();
        CallEncoder::new(seq, &target_id, &method, args).encode(&mut enc).unwrap();
        let payload = enc.into_bytes().unwrap();

        let transport = transport.clone();
        let sig = signature.clone();

        Box::new(async move {
            let reply_vals = Proxy::invoke(&payload, &transport, &sig).await
                .map_err(map_proxy_error)?;

            if reply_vals.len() != results.len() {
                return Err(wasmtime::Error::msg(format!(
                    "Result count mismatch: expected {}, got {}",
                    results.len(),
                    reply_vals.len()
                )));
            }

            for (i, val) in reply_vals.into_iter().enumerate() {
                results[i] = val;
            }

            Ok(())
        })
    }).map_err(Error::Wasmtime)?;

    Ok(())
}

/// Maps domain-specific Proxy errors to Wasmtime errors for traps.
fn map_proxy_error(e: ProxyError) -> wasmtime::Error {
    // In a production system, we might want to attach more structured data here
    // or map specific remote failures (like AppTrapped) to specific Wasm traps.
    // For now, we propagate the error description.
    wasmtime::Error::msg(format!("Remote Call Failed: {}", e))
}

// --- Error Definitions ---

#[derive(Debug)]
pub enum Error {
    /// The interface requested for linking was not found in the Ledger.
    InterfaceNotFound(String),
    /// The root function requested for linking was not found in the Ledger.
    FunctionNotFound(String),
    /// Wasmtime linker error (e.g., duplicate definition, shadow disabled).
    Wasmtime(wasmtime::Error),
    /// Result count mismatch between signature and proxy response.
    ResultCountMismatch { expected: usize, got: usize },
    /// Proxy invocation failed.
    Proxy(ProxyError),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InterfaceNotFound(name) => write!(f, "Interface '{}' not found in Ledger", name),
            Self::FunctionNotFound(name) => write!(f, "Function '{}' not found in Ledger", name),
            Self::Wasmtime(e) => write!(f, "Wasmtime linker error: {}", e),
            Self::ResultCountMismatch { expected, got } => {
                write!(f, "Result count mismatch: expected {}, got {}", expected, got)
            }
            Self::Proxy(e) => write!(f, "Proxy error: {}", e),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Proxy(e) => Some(e),
            _ => None,
        }
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;

    use neopack::Decoder;
    use neopack::Encoder;
    use neorpc::ReplyOkEncoder;
    use neorpc::RpcFrame;
    use wasmtime::component::Component;
    use wasmtime::Engine;
    use wasmtime::Store;

    // --- Mocks ---

    struct MockTransport {
        // Simplified: echo back success with empty results for any call
        // Real tests would inspect payload
    }

    #[async_trait::async_trait]
    impl Transport for MockTransport {
        async fn call(&self, payload: &[u8]) -> crate::transport::Result<Vec<u8>> {
            // Decode the call to get the sequence number
            let mut dec = Decoder::new(payload);
            let frame = RpcFrame::decode(&mut dec).expect("Mock received invalid frame");

            let seq = match frame {
                RpcFrame::Call(c) => c.seq,
                _ => panic!("Mock received non-call frame"),
            };

            // Respond with Success (empty results)
            let mut enc = Encoder::new();
            ReplyOkEncoder::new(seq, &[]).encode(&mut enc).unwrap();
            Ok(enc.into_bytes().unwrap())
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
        let transport = Arc::new(MockTransport {});
        let target = RemoteTarget {
            transport,
            target_id: "service-1".to_string(),
        };

        // Perform the binding
        Binder::link_remote_interface(
            &mut linker,
            &ledger,
            "my:service/api",
            target
        ).expect("Binding failed");

        // Instantiate and run to verify the closure works
        let mut store = Store::new(&engine, ExorunCtx::new());
        let instance = linker.instantiate_async(&mut store, &component).await.expect("Instantiation failed");

        let run = instance.get_typed_func::<(), ()>(&mut store, "run").expect("Get func failed");
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
            transport: Arc::new(MockTransport {}),
            target_id: "s".into(),
        };

        let err = Binder::link_remote_interface(
            &mut linker,
            &ledger,
            "missing:interface",
            target
        ).unwrap_err();

        match err {
            Error::InterfaceNotFound(name) => assert_eq!(name, "missing:interface"),
            _ => panic!("Wrong error type"),
        }
    }
}
