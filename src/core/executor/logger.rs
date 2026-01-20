use crate::core::processor::processor::Opportunity;
use anyhow::Result;

#[async_trait::async_trait]
pub trait OpportunityLogger: Send + Sync {
    async fn log_opportunity(&self, opportunity_id: &str, opportunity: &Opportunity) -> Result<()>;

    async fn log_opportunity_again(
        &self,
        opportunity_id: &str,
        opportunity: &Opportunity,
    ) -> Result<()>;
}

#[async_trait::async_trait]
pub trait NotificationLogger: Send + Sync {
    async fn log_notification(&self, notification: &str) -> Result<()>;
    async fn log_error(&self, error: &str) -> Result<()>;
    async fn log_error_with_chain_id(&self, chain_id: u64, error: &str) -> Result<()>;
}
