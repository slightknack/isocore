//! # Ledger of component interfaces and required capabilities
//!
//! The Ledger is the source of truth for the capabilities of a Component.
//! It maps the abstract intent of a WIT interface
//! to the concrete schema required for RPC serialization.
//!
//! ## Philosophy
//!
//! - **Link-Time Safety**: We validate that interfaces are "wire-safe" (contain no resources) at creation time.
//! - **Schema Registry**: We store `wasmtime::component::Type` handles, allowing O(1) lookup during the hot path of an RPC call.

use std::collections::HashMap;

use wasmtime::component::types::ComponentFunc;
use wasmtime::component::types::ComponentItem;
use wasmtime::component::Component;
use wasmtime::component::Type;

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
            Error::ResourceNotWireSafe => write!(f, "resources cannot cross network boundaries"),
            Error::FutureNotWireSafe => write!(f, "futures cannot cross network boundaries"),
            Error::StreamNotWireSafe => write!(f, "streams cannot cross network boundaries"),
            Error::ErrorContextNotWireSafe => write!(f, "error contexts cannot cross network boundaries"),
            Error::InvalidParameter { import_name, details } => write!(f, "import '{}' is not wire-safe: parameter contains forbidden type: {}", import_name, details),
            Error::InvalidResult { import_name, details } => write!(f, "import '{}' is not wire-safe: result contains forbidden type: {}", import_name, details),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// A registry of all imported and exported interfaces for a component.
///
/// This enables bidirectional validation: we can check that when linking,
/// the importer's requirements match the exporter's capabilities.
#[derive(Clone, Debug)]
pub struct Ledger {
    pub imports: HashMap<String, InterfaceSchema>,
    pub exports: HashMap<String, InterfaceSchema>,
}

impl Ledger {
    /// Introspects a Component to build a Ledger of its imports and exports.
    ///
    /// Only includes interfaces that are wire-safe (no resources, futures, or streams).
    /// Interfaces that aren't wire-safe are silently skipped, as they may be host
    /// components or local-only interfaces.
    pub fn from_component(component: &Component) -> Result<Self> {
        let engine = component.engine();
        let comp_ty = component.component_type();

        let imports = Self::extract_interfaces(engine, comp_ty.imports(engine))?;
        let exports = Self::extract_interfaces(engine, comp_ty.exports(engine))?;

        Ok(Self { imports, exports })
    }

    /// Extracts wire-safe interfaces from an iterator of component items.
    fn extract_interfaces<'a>(
        engine: &wasmtime::Engine,
        items: impl Iterator<Item = (&'a str, ComponentItem)>,
    ) -> Result<HashMap<String, InterfaceSchema>> {
        let mut interfaces = HashMap::new();

        for (name, item) in items {
            let ComponentItem::ComponentInstance(inst_ty) = item else { continue };

            // Try to extract the interface schema - skip if it's not wire-safe
            let interface = match InterfaceSchema::from_inst_ty(engine, name, inst_ty) {
                Ok(schema) => schema,
                Err(_) => continue, // Not wire-safe, skip it
            };

            if interface.funcs.is_empty() { continue; }
            interfaces.insert(name.to_string(), interface);
        }

        Ok(interfaces)
    }

    /// Looks up the signature for an interface method in imports.
    pub fn get_interface_func(&self, interface: &str, method: &str) -> Option<&FuncSignature> {
        self.imports.get(interface).and_then(|i| i.funcs.get(method))
    }
}

/// Validates that an import interface is compatible with an export interface.
///
/// Checks that:
/// - Every function in the import exists in the export
/// - Parameter counts match
/// - Result counts match
///
/// Note: We don't do deep type checking here because wasmtime will validate
/// types during instantiation. This validation provides early, clear errors.
pub fn validate_compatibility(
    interface_name: &str,
    import: &InterfaceSchema,
    export: &InterfaceSchema,
) -> Result<()> {
    for (func_name, import_sig) in &import.funcs {
        let export_sig = export.funcs.get(func_name).ok_or_else(|| {
            Error::InvalidParameter {
                import_name: format!("{}#{}", interface_name, func_name),
                details: format!("function not found in target's exports"),
            }
        })?;

        // Validate parameter count
        if import_sig.params.len() != export_sig.params.len() {
            return Err(Error::InvalidParameter {
                import_name: format!("{}#{}", interface_name, func_name),
                details: format!(
                    "parameter count mismatch: import expects {}, export provides {}",
                    import_sig.params.len(),
                    export_sig.params.len()
                ),
            });
        }

        // Validate result count
        if import_sig.results.len() != export_sig.results.len() {
            return Err(Error::InvalidResult {
                import_name: format!("{}#{}", interface_name, func_name),
                details: format!(
                    "result count mismatch: import expects {}, export provides {}",
                    import_sig.results.len(),
                    export_sig.results.len()
                ),
            });
        }
    }

    Ok(())
}

/// The schema for a named interface (e.g., "wasi:filesystem/types").
#[derive(Clone, Debug)]
pub struct InterfaceSchema {
    pub funcs: HashMap<String, FuncSignature>,
}

impl InterfaceSchema {
    /// Extracts all exported functions from a ComponentInstance.
    ///
    /// Validates that all function signatures are wire-safe (no resources, futures, or streams).
    fn from_inst_ty(engine: &wasmtime::Engine, name: &str, inst_ty: wasmtime::component::types::ComponentInstance) -> Result<Self> {
        let mut funcs = HashMap::new();

        for (func_name, func_item) in inst_ty.exports(engine) {
            let ComponentItem::ComponentFunc(func_ty) = func_item else { continue };
            let import_name = format!("{name}#{func_name}");
            let sig = FuncSignature::from_func_ty(&func_ty, &import_name)?;
            funcs.insert(func_name.to_string(), sig);
        }

        Ok(Self { funcs })
    }
}

/// The type signature of a specific function.
#[derive(Clone, Debug)]
pub struct FuncSignature {
    pub params: Vec<Type>,
    pub results: Vec<Type>,
}

impl FuncSignature {
    /// Extracts and validates a function signature from a ComponentFunc.
    ///
    /// Validates that all parameter and result types are wire-safe.
    /// Returns detailed errors with the import name for context.
    fn from_func_ty(func_ty: &ComponentFunc, import_name: &str) -> Result<Self> {
        let params: Vec<Type> = func_ty.params()
            .map(|(_, ty)| check_wire_safe(ty))
            .collect::<Result<Vec<_>>>()
            .map_err(|e| Error::InvalidParameter {
                import_name: import_name.to_string(),
                details: e.to_string(),
            })?;

        let results: Vec<Type> = func_ty.results()
            .map(|ty| check_wire_safe(ty))
            .collect::<Result<Vec<_>>>()
            .map_err(|e| Error::InvalidResult {
                import_name: import_name.to_string(),
                details: e.to_string(),
            })?;

        Ok(Self { params, results })
    }
}

/// Recursively validates that a type is pure data and serializable.
fn check_wire_safe(ty: Type) -> Result<Type> {
    match &ty {
        // Unserializable
        Type::Own(_)       => return Err(Error::ResourceNotWireSafe),
        Type::Borrow(_)    => return Err(Error::ResourceNotWireSafe),
        Type::Future(_)    => return Err(Error::FutureNotWireSafe),
        Type::Stream(_)    => return Err(Error::StreamNotWireSafe),
        Type::ErrorContext => return Err(Error::ErrorContextNotWireSafe),

        // Scalar
        Type::Bool                                   |
        Type::U8 | Type::U16 | Type::U32 | Type::U64 |
        Type::S8 | Type::S16 | Type::S32 | Type::S64 |
        Type::Float32        | Type::Float64         |
        Type::Char           | Type::String          |
        Type::Enum(_)        | Type::Flags(_)        => (),

        // Sum
        Type::Option(h) => { check_wire_safe(h.ty())?; }
        Type::Result(h) => {
            h.ok().map_or(Ok(()), |t| check_wire_safe(t).map(|_| ()))?;
            h.err().map_or(Ok(()), |t| check_wire_safe(t).map(|_| ()))?;
        }
        Type::Variant(h) => {
            h.cases().try_for_each(|c| c.ty.map_or(Ok(()), |t| check_wire_safe(t).map(|_| ())))?;
        }

        // Product
        Type::List(h) => { check_wire_safe(h.ty())?; }
        Type::Tuple(h) => {
            h.types().try_for_each(|t| check_wire_safe(t).map(|_| ()))?;
        }
        Type::Record(h) => {
            h.fields().try_for_each(|f| check_wire_safe(f.ty.clone()).map(|_| ()))?;
        }
    }
    Ok(ty)
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
                (import "logger" (instance $logger
                    (export "log" (func (param "msg" string) (result u32)))
                ))
            )
        "#);

        let ledger = Ledger::from_component(&c).expect("Ledger creation failed");
        let sig = ledger.get_interface_func("logger", "log").expect("logger.log not found");

        assert_eq!(sig.params.len(), 1);
        assert_eq!(sig.results.len(), 1);
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
        let c = compile(r#"
            (component
                (type $t (instance
                    (export "r" (type (sub resource)))
                ))
                (import "inst" (instance $i (type $t)))
                (alias export $i "r" (type $r))
                (import "bad" (instance $bad
                    (export "use-resource" (func (param "h" (borrow $r))))
                ))
            )
        "#);

        let ledger = Ledger::from_component(&c).unwrap();
        // Interface with resources should be skipped, not included in the ledger
        assert!(ledger.imports.is_empty());
        assert!(ledger.get_interface_func("bad", "use-resource").is_none());
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
                (import "bad" (instance $bad
                    (export "get-resource" (func (result (own $r))))
                ))
            )
        "#);

        let ledger = Ledger::from_component(&c).unwrap();
        // Interface with resources in results should be skipped
        assert!(ledger.imports.is_empty());
        assert!(ledger.get_interface_func("bad", "get-resource").is_none());
    }

    #[test]
    fn test_ledger_rejects_nested_resources() {
        let c = compile(r#"
            (component
                (type $t (instance
                    (export "r" (type (sub resource)))
                ))
                (import "inst" (instance $i (type $t)))
                (alias export $i "r" (type $r))
                (import "bad" (instance $bad
                    (export "process-list" (func (param "nested" (list (own $r)))))
                ))
            )
        "#);

        let ledger = Ledger::from_component(&c).unwrap();
        // Interface with nested resources should be skipped
        assert!(ledger.imports.is_empty());
        assert!(ledger.get_interface_func("bad", "process-list").is_none());
    }

    #[test]
    fn test_ledger_allows_complex_pure_data() {
        let c = compile(r#"
            (component
                (import "good" (instance $good
                    (export "process" (func
                        (param "opt" (option string))
                        (param "res" (result u32 (error string)))
                        (param "lst" (list u32))
                        (param "tup" (tuple string u32))
                    ))
                ))
            )
        "#);

        Ledger::from_component(&c).expect("Should accept pure complex data");
    }
}
