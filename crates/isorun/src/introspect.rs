//! Component introspection and metadata extraction.
//!
//! This module provides utilities for discovering exported functions and their
//! signatures from Wasmtime component instances at runtime.

use anyhow::Result;

use wasmtime::component::Component;
use wasmtime::component::types::ComponentItem;

/// Information about an exported or imported function.
#[derive(Debug, Clone)]
pub struct FuncInfo {
    /// The full name of the function (e.g., "test:demo/math#add").
    pub name: String,
    /// The interface name if this is part of an interface (e.g., "test:demo/math").
    pub interface: Option<String>,
    /// The function name within the interface (e.g., "add").
    pub func_name: String,
}

// Backwards compatibility alias
pub type ExportedFunc = FuncInfo;

/// Discover all exported functions from a component.
///
/// This walks the component's type information and extracts function metadata
/// that can be used for dynamic linking.
///
/// # Example
///
/// ```rust,no_run
/// # use isorun::introspect::discover_exports;
/// # use wasmtime::component::Component;
/// # fn example(component: &Component) -> anyhow::Result<()> {
/// let exports = discover_exports(component)?;
/// for export in exports {
///     println!("Found: {} in {:?}", export.func_name, export.interface);
/// }
/// # Ok(())
/// # }
/// ```
pub fn discover_exports(component: &Component) -> Result<Vec<ExportedFunc>> {
    let mut exports = Vec::new();
    
    let engine = component.engine();
    let component_ty = component.component_type();
    
    // Walk through all exports
    for (name, item) in component_ty.exports(engine) {
        match item {
            ComponentItem::ComponentFunc(_func_ty) => {
                // Direct function export (not in an interface)
                exports.push(ExportedFunc {
                    name: name.to_string(),
                    interface: None,
                    func_name: name.to_string(),
                });
            }
            ComponentItem::ComponentInstance(inst_ty) => {
                // Interface export (e.g., "test:demo/math")
                let interface_name = name.to_string();
                
                for (func_name, func_item) in inst_ty.exports(engine) {
                    if matches!(func_item, ComponentItem::ComponentFunc(_)) {
                        let full_name = format!("{}#{}", interface_name, func_name);
                        exports.push(ExportedFunc {
                            name: full_name,
                            interface: Some(interface_name.clone()),
                            func_name: func_name.to_string(),
                        });
                    }
                }
            }
            _ => {
                // Ignore non-function exports
            }
        }
    }
    
    Ok(exports)
}

/// Discover all imported functions from a component.
///
/// This walks the component's import requirements and extracts function metadata
/// that can be used to determine what needs to be linked.
///
/// # Example
///
/// ```rust,no_run
/// # use isorun::introspect::discover_imports;
/// # use wasmtime::component::Component;
/// # fn example(component: &Component) -> anyhow::Result<()> {
/// let imports = discover_imports(component)?;
/// for import in imports {
///     println!("Needs: {} from {:?}", import.func_name, import.interface);
/// }
/// # Ok(())
/// # }
/// ```
pub fn discover_imports(component: &Component) -> Result<Vec<FuncInfo>> {
    let mut imports = Vec::new();
    
    let engine = component.engine();
    let component_ty = component.component_type();
    
    // Walk through all imports
    for (name, item) in component_ty.imports(engine) {
        match item {
            ComponentItem::ComponentFunc(_func_ty) => {
                // Direct function import (not in an interface)
                imports.push(FuncInfo {
                    name: name.to_string(),
                    interface: None,
                    func_name: name.to_string(),
                });
            }
            ComponentItem::ComponentInstance(inst_ty) => {
                // Interface import (e.g., "test:demo/math")
                let interface_name = name.to_string();
                
                for (func_name, func_item) in inst_ty.exports(engine) {
                    if matches!(func_item, ComponentItem::ComponentFunc(_)) {
                        let full_name = format!("{}#{}", interface_name, func_name);
                        imports.push(FuncInfo {
                            name: full_name,
                            interface: Some(interface_name.clone()),
                            func_name: func_name.to_string(),
                        });
                    }
                }
            }
            _ => {
                // Ignore non-function imports
            }
        }
    }
    
    Ok(imports)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exported_func_creation() {
        let export = ExportedFunc {
            name: "test:demo/math#add".to_string(),
            interface: Some("test:demo/math".to_string()),
            func_name: "add".to_string(),
        };
        
        assert_eq!(export.name, "test:demo/math#add");
        assert_eq!(export.interface, Some("test:demo/math".to_string()));
        assert_eq!(export.func_name, "add");
    }
}
