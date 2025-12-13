use anyhow::Result;
use wasmtime::component::Component;
use wasmtime::component::types::ComponentItem;
use wasmtime::component::Type;

#[derive(Debug, Clone)]
pub struct FuncInfo {
    pub name: String,
    pub interface: Option<String>,
    pub func_name: String,
    pub params: Vec<Type>,
    pub results: Vec<Type>,
}

pub type ExportedFunc = FuncInfo;

pub fn discover_exports(component: &Component) -> Result<Vec<ExportedFunc>> {
    let mut exports = Vec::new();
    let engine = component.engine();
    let component_ty = component.component_type();
    
    for (name, item) in component_ty.exports(engine) {
        match item {
            ComponentItem::ComponentFunc(func_ty) => {
                exports.push(ExportedFunc {
                    name: name.to_string(),
                    interface: None,
                    func_name: name.to_string(),
                    params: func_ty.params().map(|p| p.ty).collect(),
                    results: func_ty.results().map(|p| p.ty).collect(),
                });
            }
            ComponentItem::ComponentInstance(inst_ty) => {
                let interface_name = name.to_string();
                for (func_name, func_item) in inst_ty.exports(engine) {
                    if let ComponentItem::ComponentFunc(func_ty) = func_item {
                        let full_name = format!("{}#{}", interface_name, func_name);
                        exports.push(ExportedFunc {
                            name: full_name,
                            interface: Some(interface_name.clone()),
                            func_name: func_name.to_string(),
                            params: func_ty.params().map(|p| p.ty).collect(),
                            results: func_ty.results().map(|p| p.ty).collect(),
                        });
                    }
                }
            }
            _ => {}
        }
    }
    Ok(exports)
}

pub fn discover_imports(component: &Component) -> Result<Vec<FuncInfo>> {
    let mut imports = Vec::new();
    let engine = component.engine();
    let component_ty = component.component_type();
    
    for (name, item) in component_ty.imports(engine) {
        match item {
            ComponentItem::ComponentFunc(func_ty) => {
                imports.push(FuncInfo {
                    name: name.to_string(),
                    interface: None,
                    func_name: name.to_string(),
                    params: func_ty.params().map(|p| p.ty).collect(),
                    results: func_ty.results().map(|p| p.ty).collect(),
                });
            }
            ComponentItem::ComponentInstance(inst_ty) => {
                let interface_name = name.to_string();
                for (func_name, func_item) in inst_ty.exports(engine) {
                    if let ComponentItem::ComponentFunc(func_ty) = func_item {
                        let full_name = format!("{}#{}", interface_name, func_name);
                        imports.push(FuncInfo {
                            name: full_name,
                            interface: Some(interface_name.clone()),
                            func_name: func_name.to_string(),
                            params: func_ty.params().map(|p| p.ty).collect(),
                            results: func_ty.results().map(|p| p.ty).collect(),
                        });
                    }
                }
            }
            _ => {}
        }
    }
    Ok(imports)
}
