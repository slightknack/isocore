
use wasmtime::component::ResourceTable;

pub struct ContextBuilder;

impl ContextBuilder {
    pub fn new() -> Self {
        Self
    }
}

pub struct IsorunCtx {
    pub(crate) table: ResourceTable,
}

impl IsorunCtx {
    pub fn new(_builder: ContextBuilder) -> Self {
        Self {
            table: ResourceTable::new(),
        }
    }
}
