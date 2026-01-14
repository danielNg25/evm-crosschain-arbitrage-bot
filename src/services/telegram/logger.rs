use log::info;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{
    core::{
        processor::processor::Opportunity, ErrorLogger, MultichainNetworkRegistry,
        OpportunityLogger,
    },
    services::TelegramService,
};
use anyhow::Result;
use std::time::{Duration, Instant};

pub struct TelegramLogger {
    service: TelegramService,
    multichain_network_registry: Arc<MultichainNetworkRegistry>,
    error_thread_id: u64,
    error_log_interval_secs: u64,
    last_log_time: Arc<Mutex<Instant>>,
}

impl TelegramLogger {
    pub async fn new(
        service: TelegramService,
        multichain_network_registry: Arc<MultichainNetworkRegistry>,
        error_thread_id: u64,
        error_log_interval_secs: u64,
    ) -> Self {
        // Send test message to the opp thread
        service
            .send_markdown_message_to_general_channel("Init Telegram Logger")
            .await
            .unwrap();
        Self {
            service,
            multichain_network_registry,
            error_thread_id,
            error_log_interval_secs,
            last_log_time: Arc::new(Mutex::new(
                Instant::now() - Duration::from_secs(86400 * 365),
            )),
        }
    }
}

#[async_trait::async_trait]
impl OpportunityLogger for TelegramLogger {
    async fn log_opportunity(
        &self,
        _opportunity_id: &str,
        opportunity: &Opportunity,
    ) -> Result<()> {
        let (source_chain_id, source_chain_name) = self
            .multichain_network_registry
            .get_network_info(opportunity.path.source_chain_id)
            .await
            .unwrap();

        let (target_chain_id, target_chain_name) = self
            .multichain_network_registry
            .get_network_info(opportunity.path.target_chain_id)
            .await
            .unwrap();

        let source_token_symbol = self
            .multichain_network_registry
            .get_token_symbol(source_chain_id, opportunity.path.source_chain[0].token_in)
            .await
            .unwrap();
        let target_token_symbol = self
            .multichain_network_registry
            .get_token_symbol(
                target_chain_id,
                opportunity.path.target_chain.last().unwrap().token_out,
            )
            .await
            .unwrap();
        let anchor_token_symbol = self
            .multichain_network_registry
            .get_token_symbol(source_chain_id, opportunity.path.anchor_token)
            .await
            .unwrap();
        let amount_in = self
            .multichain_network_registry
            .to_human_amount(
                source_chain_id,
                opportunity.path.source_chain[0].token_in,
                opportunity.amount_in,
            )
            .await
            .unwrap();
        let amount_out = self
            .multichain_network_registry
            .to_human_amount(
                target_chain_id,
                opportunity.path.target_chain.last().unwrap().token_out,
                opportunity.amount_out,
            )
            .await
            .unwrap();
        let anchor_token_amount = self
            .multichain_network_registry
            .to_human_amount(
                source_chain_id,
                opportunity.path.anchor_token,
                opportunity.anchor_token_amount,
            )
            .await
            .unwrap();

        let message = format!(
            "💰 *{}*: _{}_ -> _{}_\nProfit: _{}_$\n*{}* in: _{}_\n*{}*: _{}_\n*{}* out: _{}_",
            anchor_token_symbol,
            source_chain_name,
            target_chain_name,
            opportunity.profit,
            source_token_symbol,
            amount_in,
            anchor_token_symbol,
            anchor_token_amount,
            target_token_symbol,
            amount_out
        );

        self.service
            .send_markdown_message_to_general_channel(&message)
            .await
    }

    async fn log_opportunity_again(
        &self,
        opportunity_id: &str,
        opportunity: &Opportunity,
    ) -> Result<()> {
        self.log_opportunity(opportunity_id, opportunity).await
    }
}

#[async_trait::async_trait]
impl ErrorLogger for TelegramLogger {
    async fn log_error(&self, error: &str) -> Result<()> {
        info!("Logging error: {}", error);
        self.service
            .send_markdown_message_to_thread(&format!("🔴 _{}_", error), self.error_thread_id)
            .await
    }

    async fn log_error_with_chain_id(&self, chain_id: u64, error: &str) -> Result<()> {
        if self.last_log_time.lock().await.elapsed()
            < Duration::from_secs(self.error_log_interval_secs)
        {
            return Ok(());
        }
        *self.last_log_time.lock().await = Instant::now();
        let message = format!("🔴 CHAIN ID *{}*: _{}_", chain_id, error);
        self.service
            .send_markdown_message_to_thread(&message, self.error_thread_id)
            .await
    }
}
