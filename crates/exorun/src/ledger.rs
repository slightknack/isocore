//! # The Static Ledger
//!
//! The Ledger is the source of truth for the capabilities of a Component.
//! It maps the abstract intent of a WIT interface to the concrete schema
//! required for RPC serialization.
//!
//! ## Philosophy
//!
//! - **Link-Time Safety**: We validate that interfaces are "wire-safe" (no resources)
//!   at creation time, not call time.
//! - **Schema Registry**: We store `wasmtime::component::Type` handles, keyed by
//!   Interface and Method, allowing O(1) lookup during the hot path of an RPC call.
//! - **Engine Coupling**: The Ledger holds `Type` handles which keep the
//!   Wasmtime `Engine` alive. This is intentional.

use std::collections::HashMap;
use wasmtime::component::{Component, Type};
use wasmtime::component::types::{ComponentItem, ComponentFunc};

/// Ledger errors.
#[derive(Debug, Clone)]
pub enum Error {
    /// Type contains resources which cannot cross network boundaries.
    ResourceNotWireSafe,
    /// Type contains futures which cannot cross network boundaries.
    FutureNotWireSafe,
    /// Type contains streams which cannot cross network boundaries.
    StreamNotWireSafe,
    /// Type contains error contexts which cannot cross network boundaries.
    ErrorContextNotWireSafe,
    /// Parameter contains forbidden type.
    InvalidParameter { import_name: String, details: String },
    /// Result contains forbidden type.
    InvalidResult { import_name: String, details: String },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::ResourceNotWireSafe => write!(f, "Resources cannot cross network boundaries"),
            Error::FutureNotWireSafe => write!(f, "Futures cannot cross network boundaries"),
            Error::StreamNotWireSafe => write!(f, "Streams cannot cross network boundaries"),
            Error::ErrorContextNotWireSafe => write!(f, "Error Contexts cannot cross network boundaries"),
            Error::InvalidParameter { import_name, details } => {
                write!(f, "Import '{}' is not wire-safe: parameter contains forbidden type: {}", import_name, details)
            }
            Error::InvalidResult { import_name, details } => {
                write!(f, "Import '{}' is not wire-safe: result contains forbidden type: {}", import_name, details)
            }
        }
    }
}

impl std::error::Error for Error {}

/// Specialized `Result` for Ledger operations.
pub type Result<T> = std::result::Result<T, Error>;

/// A registry of all imported interfaces and functions for a component.
#[derive(Clone, Debug)]
pub struct Ledger {
    /// Imports that look like `(import "name" (func ...))`
    pub root_funcs: HashMap<String, FunctionSignature>,
    /// Imports that look like `(import "interface" (instance ...))`
    pub interfaces: HashMap<String, InterfaceSchema>,
}

/// The schema for a named interface (e.g., "wasi:filesystem/types").
#[derive(Clone, Debug)]
pub struct InterfaceSchema {
    pub funcs: HashMap<String, FunctionSignature>,
}

/// The type signature of a specific function.
///
/// This is the "Shape" we need to coerce `Val`s into.
#[derive(Clone, Debug)]
pub struct FunctionSignature {
    pub params: Vec<Type>,
    pub results: Vec<Type>,
}

impl Ledger {
    /// Introspects a Component to build a Ledger of its imports.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The component imports resources, futures, or streams (which cannot cross the wire).
    /// - The component structure is malformed/unsupported.
    pub fn from_component(component: &Component) -> Result<Self> {
        let mut root_funcs = HashMap::new();
        let mut interfaces = HashMap::new();

        let engine = component.engine();

        // We walk the imports. In the Component Model, imports define the "Client" view.
        // This is what we need to stub out for Remote calls.
        for (name, item) in component.component_type().imports(engine) {
            match item {
                ComponentItem::ComponentFunc(func_ty) => {
                    let sig = FunctionSignature::from_func_ty(&func_ty, name)?;
                    root_funcs.insert(name.to_string(), sig);
                }
                ComponentItem::ComponentInstance(inst_ty) => {
                    let mut funcs = HashMap::new();
                    for (func_name, func_item) in inst_ty.exports(engine) {
                        if let ComponentItem::ComponentFunc(func_ty) = func_item {
                            let import_name = format!("{name}#{func_name}");
                            let sig = FunctionSignature::from_func_ty(&func_ty, &import_name)?;
                            funcs.insert(func_name.to_string(), sig);
                        }
                        // We ignore types/modules exported by instances, we only care about callables.
                    }
                    if !funcs.is_empty() {
                        interfaces.insert(name.to_string(), InterfaceSchema { funcs });
                    }
                }
                _ => {
                    // We ignore imported Modules, Types, or Resources (unless used in funcs).
                    // We only care about things we have to *execute*.
                }
            }
        }

        Ok(Self { root_funcs, interfaces })
    }

    /// Looks up the signature for a root import.
    pub fn get_root_func(&self, name: &str) -> Option<&FunctionSignature> {
        self.root_funcs.get(name)
    }

    /// Looks up the signature for an interface method.
    pub fn get_interface_func(&self, interface: &str, method: &str) -> Option<&FunctionSignature> {
        self.interfaces.get(interface).and_then(|i| i.funcs.get(method))
    }
}

impl FunctionSignature {
    fn from_func_ty(func_ty: &ComponentFunc, import_name: &str) -> Result<Self> {
        let params: Vec<Type> = func_ty.params().map(|(_, ty)| ty).collect();
        let results: Vec<Type> = func_ty.results().collect();

        // VALIDATION: The "No-Resource" Invariant.
        for ty in &params {
            validate_wire_safe(ty).map_err(|e| Error::InvalidParameter {
                import_name: import_name.to_string(),
                details: e.to_string(),
            })?;
        }
        for ty in &results {
            validate_wire_safe(ty).map_err(|e| Error::InvalidResult {
                import_name: import_name.to_string(),
                details: e.to_string(),
            })?;
        }

        Ok(Self { params, results })
    }
}

/// Recursively validates that a type is pure data and serializable.
///
/// Returns Err if the type contains Resources, Futures, or Streams.
fn validate_wire_safe(ty: &Type) -> Result<()> {
    match ty {
        Type::Bool | Type::U8 | Type::U16 | Type::U32 | Type::U64 |
        Type::S8 | Type::S16 | Type::S32 | Type::S64 |
        Type::Float32 | Type::Float64 | Type::Char | Type::String => Ok(()),

        Type::List(h) => validate_wire_safe(&h.ty()),

        Type::Tuple(h) => {
            for t in h.types() { validate_wire_safe(&t)?; }
            Ok(())
        },

        Type::Option(h) => validate_wire_safe(&h.ty()),

        Type::Result(h) => {
            if let Some(ok) = h.ok() { validate_wire_safe(&ok)?; }
            if let Some(err) = h.err() { validate_wire_safe(&err)?; }
            Ok(())
        },

        Type::Record(h) => {
            for field in h.fields() { validate_wire_safe(&field.ty)?; }
            Ok(())
        },

        Type::Variant(h) => {
            for case in h.cases() {
                if let Some(t) = case.ty { validate_wire_safe(&t)?; }
            }
            Ok(())
        },

        // Enums and Flags are always pure scalars/bitmaps.
        Type::Enum(_) | Type::Flags(_) => Ok(()),

        // THE FORBIDDEN ZONE
        Type::Own(_) | Type::Borrow(_) => Err(Error::ResourceNotWireSafe),
        Type::Future(_) => Err(Error::FutureNotWireSafe),
        Type::Stream(_) => Err(Error::StreamNotWireSafe),
        Type::ErrorContext => Err(Error::ErrorContextNotWireSafe),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasmtime::Engine;

    fn compile(wat: &str) -> Component {
        let engine = Engine::default();
        Component::new(&engine, wat).unwrap()
    }

    #[test]
    fn test_ledger_discovery_scalars() {
        let c = compile(r#"
            (component
                (import "logger" (func $log (param "msg" string) (result u32)))
            )
        "#);

        let ledger = Ledger::from_component(&c).expect("Ledger creation failed");
        let sig = ledger.get_root_func("logger").expect("logger not found");

        assert_eq!(sig.params.len(), 1); // string
        assert_eq!(sig.results.len(), 1); // u32
    }

    #[test]
    fn test_ledger_discovery_interfaces() {
        let c = compile(r#"
            (component
                (type $t (instance
                    (export "get" (func (param "k" string) (result string)))
                    (export "set" (func (param "k" string) (param "v" string)))
                ))
                (import "kv" (instance (type $t)))
            )
        "#);

        let ledger = Ledger::from_component(&c).expect("Ledger creation failed");
        let sig_get = ledger.get_interface_func("kv", "get").expect("kv.get not found");

        assert_eq!(sig_get.params.len(), 1);
        assert_eq!(sig_get.results.len(), 1);
    }

    #[test]
    fn test_ledger_rejects_resources_in_params() {
        // A component that imports a function taking a resource handle
        let c = compile(r#"
            (component
                (type $t (instance
                    (export "r" (type (sub resource)))
                ))
                (import "inst" (instance $i (type $t)))
                (alias export $i "r" (type $r))
                (import "bad" (func (param "h" (borrow $r))))
            )
        "#);

        let err = Ledger::from_component(&c).unwrap_err();
        assert!(format!("{err}").contains("not wire-safe"));
        assert!(format!("{err}").contains("Resources cannot cross"));
    }

    #[test]
    fn test_ledger_rejects_resources_in_results() {
        let c = compile(r#"
            (component
                (type $t (instance
                    (export "r" (type (sub resource)))
                ))
                (import "inst" (instance $i (type $t)))
                (alias export $i "r" (type $r))
                (import "bad" (func (result (own $r))))
            )
        "#);

        let err = Ledger::from_component(&c).unwrap_err();
        assert!(format!("{err}").contains("not wire-safe"));
    }

    #[test]
    fn test_ledger_rejects_nested_resources() {
        // Resource inside a List (nested in a complex type)
        let c = compile(r#"
            (component
                (type $t (instance
                    (export "r" (type (sub resource)))
                ))
                (import "inst" (instance $i (type $t)))
                (alias export $i "r" (type $r))
                (import "bad" (func (param "nested" (list (own $r)))))
            )
        "#);

        let err = Ledger::from_component(&c).unwrap_err();
        assert!(format!("{err}").contains("not wire-safe"));
    }

    #[test]
    fn test_ledger_allows_complex_pure_data() {
        // Test with complex types that are supported: list, option, result, tuple
        let c = compile(r#"
            (component
                (import "good" (func 
                    (param "opt" (option string))
                    (param "res" (result u32 (error string)))
                    (param "lst" (list u32))
                    (param "tup" (tuple string u32))
                ))
            )
        "#);

        Ledger::from_component(&c).expect("Should accept pure complex data");
    }
}
