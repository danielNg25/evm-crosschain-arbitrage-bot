use std::sync::Arc;

use crate::{
    core::{processor::processor::Opportunity, Logger, MultichainNetworkRegistry},
    services::TelegramService,
};
use anyhow::Result;
pub struct TelegramLogger {
    service: TelegramService,
    multichain_network_registry: Arc<MultichainNetworkRegistry>,
}

impl TelegramLogger {
    pub fn new(
        service: TelegramService,
        multichain_network_registry: Arc<MultichainNetworkRegistry>,
    ) -> Self {
        Self {
            service,
            multichain_network_registry,
        }
    }
}

#[async_trait::async_trait]
impl Logger for TelegramLogger {
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
