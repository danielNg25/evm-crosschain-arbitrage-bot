use crate::core::processor::processor::Opportunity;
use anyhow::Result;

#[async_trait::async_trait]
pub trait Logger: Send + Sync {
    async fn log_opportunity(&self, opportunity_id: &str, opportunity: &Opportunity) -> Result<()>;

    async fn log_opportunity_again(
        &self,
        opportunity_id: &str,
        opportunity: &Opportunity,
    ) -> Result<()>;
}
