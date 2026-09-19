use async_trait::async_trait;

use crate::domain::models::ReportBody;

#[async_trait]
pub trait ReportClient: Send + Sync {
    async fn report(&self, job_id: &str, body: &ReportBody) -> Result<(), String>;
}
