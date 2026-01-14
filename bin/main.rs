use anyhow::Result;

use clap::Parser;
use dashmap::DashMap;
use env_logger::Env;
use evm_arb_bot::core::{Executor, ExecutorConfig, NetworkCollector};

use evm_arb_bot::core::processor::processor::{CrossChainArbitrageProcessor, ProcessorConfig};
use evm_arb_bot::services::database::service::MongoDbService;
use evm_arb_bot::services::TelegramService;

use evm_arb_bot::config::{AppConfig, RawConfig};

use evm_arb_bot::services::logger::TelegramLogger;
use evm_arb_bot::utils::metrics::Metrics;
use log::{error, LevelFilter};

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

// Example pool addresses

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long, default_value = "info")]
    log_level: String,
    #[arg(long, default_value = "default")]
    strategy: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Parse command line arguments and setup logging
    let args = Args::parse();
    let log_level = match args.log_level.to_lowercase().as_str() {
        "error" => LevelFilter::Error,
        "warn" => LevelFilter::Warn,
        "info" => LevelFilter::Info,
        "debug" => LevelFilter::Debug,
        "trace" => LevelFilter::Trace,
        _ => LevelFilter::Info,
    };
    env_logger::Builder::from_env(Env::default().default_filter_or(log_level.to_string())).init();

    // 2. Load configuration
    let config = AppConfig::from_raw_config(RawConfig::load()?)?;

    // 3. Initialize databases
    // Initialize local database if configured

    let mongodb_service = Arc::new(match MongoDbService::new(&config.mongodb).await {
        Ok(service) => service,
        Err(e) => {
            error!("Failed to initialize MongoDB: {}", e);
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            )) as Box<dyn std::error::Error>);
        }
    });

    let mongodb_config = mongodb_service
        .get_config_repo()
        .get_or_create(
            config.processor.default_max_amount_usd,
            config.executor.default_recheck_interval,
        )
        .await?;

    let metrics = Arc::new(RwLock::new(Metrics::new()));
    let (swap_event_tx, mut swap_event_rx) = mpsc::channel(1000);
    let network_collector = Arc::new(NetworkCollector::new(
        mongodb_service.clone(),
        metrics.clone(),
        swap_event_tx,
        vec![],
    ));

    let telegram_logger = Arc::new(
        TelegramLogger::new(
            TelegramService::new(config.telegram.token, config.telegram.chat_id),
            network_collector.network_registries.clone(),
            config.telegram.error_thread_id,
            config.telegram.error_log_interval_secs,
        )
        .await,
    );

    network_collector
        .add_error_logger(telegram_logger.clone())
        .await;

    network_collector.start_mongodb_polling(config.mongodb.poll_interval_secs);

    let executing_paths = Arc::new(DashMap::new());
    let (opportunity_tx, mut opportunity_rx) = mpsc::channel(1000);
    let processor = Arc::new(CrossChainArbitrageProcessor::new(
        network_collector.network_registries.clone(),
        executing_paths.clone(),
        ProcessorConfig {
            max_iterations: config.processor.max_iterations,
            max_amount_usd: mongodb_config.max_amount_usd,
        },
        opportunity_tx.clone(),
        metrics.clone(),
    ));
    let processor_clone = processor.clone();
    tokio::spawn(async move {
        while let Some(pool) = swap_event_rx.recv().await {
            let processor_clone = processor.clone();
            tokio::spawn(async move {
                if let Err(e) = processor_clone.handle_pool_change(pool).await {
                    error!("Error handling swap event in simulator thread: {}", e);
                }
            });
        }
    });

    let executor = Arc::new(
        Executor::new(
            processor_clone,
            Arc::new(vec![telegram_logger]),
            executing_paths.clone(),
            ExecutorConfig {
                recheck_interval: mongodb_config.recheck_interval,
            },
        )
        .await,
    );

    tokio::spawn(async move {
        while let Some(opportunity) = opportunity_rx.recv().await {
            if let Err(e) = executor.execute(opportunity).await {
                error!("Error executing opportunity: {}", e);
            }
        }
    });

    // 15. Keep main thread alive with periodic database snapshot before exit
    let running = Arc::new(AtomicBool::new(true));

    while running.load(std::sync::atomic::Ordering::SeqCst) {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }

    Ok(())
}
