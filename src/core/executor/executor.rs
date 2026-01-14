use anyhow::Result;
use chrono::Utc;
use dashmap::DashMap;
use log::error;
use std::{sync::Arc, time::Duration};

use crate::core::processor::processor::CrossChainArbitrageProcessor;
use crate::core::OpportunityLogger;

use crate::core::processor::processor::Opportunity;

#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    pub recheck_interval: u64,
}

pub struct Executor {
    processor: Arc<CrossChainArbitrageProcessor>,
    loggers: Arc<Vec<Arc<dyn OpportunityLogger>>>,
    executing_paths: Arc<DashMap<String, String>>,
    config: ExecutorConfig,
}

impl Executor {
    pub async fn new(
        processor: Arc<CrossChainArbitrageProcessor>,
        loggers: Arc<Vec<Arc<dyn OpportunityLogger>>>,
        executing_paths: Arc<DashMap<String, String>>,
        config: ExecutorConfig,
    ) -> Self {
        Self {
            processor,
            loggers,
            executing_paths,
            config,
        }
    }

    pub async fn execute(&self, mut opportunity: Opportunity) -> Result<()> {
        let path_hash = opportunity.path.hash();
        if self.executing_paths.contains_key(&path_hash) {
            return Ok(());
        }

        let opportunity_id = format!("{}-{}", path_hash, Utc::now().timestamp());

        self.executing_paths
            .insert(path_hash.clone(), opportunity_id.clone());

        self.log_opportunity(&opportunity_id, &opportunity).await?;

        loop {
            tokio::time::sleep(Duration::from_millis(self.config.recheck_interval)).await;

            let result = self
                .processor
                .load_pool_and_handle_path(&opportunity.path)
                .await;

            if let Ok(Some((profit, amount_in, anchor_token_amount, amount_out, steps))) = result {
                if profit < opportunity.min_profit_usd {
                    break;
                }
                opportunity.profit = profit;
                opportunity.amount_in = amount_in;
                opportunity.amount_out = amount_out;
                opportunity.anchor_token_amount = anchor_token_amount;
                opportunity.steps = steps;

                self.log_opportunity_again(&opportunity_id, &opportunity)
                    .await?;
            } else {
                error!("Error from load_pool_and_handle_path: {:?}", result);
                break;
            }
        }

        self.executing_paths.remove(&path_hash);

        Ok(())
    }

    async fn log_opportunity(&self, opportunity_id: &str, opportunity: &Opportunity) -> Result<()> {
        for logger in self.loggers.iter() {
            let logger = Arc::clone(logger);
            let opportunity_id_clone = opportunity_id.to_string();
            let opportunity_clone = opportunity.clone();

            tokio::spawn(async move {
                if let Err(e) = logger
                    .log_opportunity(&opportunity_id_clone, &opportunity_clone)
                    .await
                {
                    error!("Logger error: {:?}", e);
                }
            });
        }

        Ok(())
    }

    async fn log_opportunity_again(
        &self,
        opportunity_id: &str,
        opportunity: &Opportunity,
    ) -> Result<()> {
        for logger in self.loggers.iter() {
            let logger = Arc::clone(logger);
            let opportunity_id_clone = opportunity_id.to_string();
            let opportunity_clone = opportunity.clone();

            tokio::spawn(async move {
                if let Err(e) = logger
                    .log_opportunity_again(&opportunity_id_clone, &opportunity_clone)
                    .await
                {
                    error!("Logger error: {:?}", e);
                }
            });
        }

        Ok(())
    }
}
