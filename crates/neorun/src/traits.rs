
use anyhow::Result;

#[async_trait::async_trait]
pub trait Transport: Send + Sync + 'static {
    async fn call(&self, payload: &[u8]) -> Result<Vec<u8>>;
}
