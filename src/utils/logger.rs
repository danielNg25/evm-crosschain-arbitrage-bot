use crate::{
    core::{
        opportunity_logger::OpportunityLogger, processor::processor::Opportunity,
        service::MongoDbService,
    },
    models::profit_token::ProfitTokenRegistry,
    utils::{
        metrics::{Metrics, OpportunityMetrics},
        utils::{log_opportunity, OpportunityStatus},
    },
};
use alloy::{primitives::Address, rpc::types::TransactionReceipt};
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct Logger {
    metrics: Arc<RwLock<Metrics>>,
    profit_token_registry: Arc<ProfitTokenRegistry>,
    opportunity_logger: Option<Arc<OpportunityLogger>>,
}

impl Logger {
    pub fn new(
        metrics: Arc<RwLock<Metrics>>,
        profit_token_registry: Arc<ProfitTokenRegistry>,
        mongodb: Option<Arc<MongoDbService>>,
        profit_receiver_address: Address,
    ) -> Self {
        Self {
            metrics,
            profit_token_registry,
            opportunity_logger: if let Some(mongodb) = mongodb {
                Some(Arc::new(OpportunityLogger::new(
                    mongodb,
                    profit_receiver_address,
                )))
            } else {
                None
            },
        }
    }

    pub async fn log_opportunity(
        &self,
        opportunity: &mut Opportunity,
        receipt: Result<TransactionReceipt>,
        metric: &OpportunityMetrics,
    ) -> Result<()> {
        if opportunity.status == OpportunityStatus::None {
            match &receipt {
                Ok(receipt) => {
                    if receipt.status() {
                        opportunity.status = OpportunityStatus::Succeeded;
                    } else {
                        opportunity.status = OpportunityStatus::Reverted;
                    }
                }
                Err(_e) => {
                    opportunity.status = OpportunityStatus::Error;
                }
            }
        };
        if let Some(opportunity_logger) = &self.opportunity_logger {
            opportunity_logger
                .log_opportunity(opportunity, &receipt, &metric, &self.profit_token_registry)
                .await?;
        }
        // else {
        //     log_opportunity(opportunity, receipt, &self.metrics).await?;
        // }
        log_opportunity(opportunity, &receipt, &self.metrics).await?;

        // Drop the opportunity from the metrics
        self.metrics
            .write()
            .await
            .drop_opportunity(opportunity.transaction_hash, opportunity.log_index);

        Ok(())
    }
}
