use async_trait::async_trait;

use crate::domain::models::HeartbeatBody;

#[async_trait]
pub trait HeartbeatClient: Send + Sync {
    async fn send(&self, body: &HeartbeatBody) -> Result<(), String>;
}
