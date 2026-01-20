use log::info;
use std::collections::HashMap;
use std::sync::Arc;
use teloxide::{
    adaptors::throttle::Throttle,
    prelude::*,
    types::{CallbackQuery, InlineKeyboardButton, InlineKeyboardMarkup},
    Bot,
};
use tokio::sync::Mutex;

use crate::{
    core::{
        processor::processor::Opportunity, MultichainNetworkRegistry, NotificationLogger,
        OpportunityLogger,
    },
    services::TelegramService,
};
use anyhow::Result;
use std::time::{Duration, Instant};

/// Format a decimal string to 4 decimal places (like toFixed(4) in TypeScript)
/// Operates on strings to avoid precision loss with large token amounts
fn format_decimal_4(value: &str) -> String {
    // Find the decimal point position
    if let Some(dot_pos) = value.find('.') {
        let integer_part = &value[..dot_pos];
        let decimal_part = &value[dot_pos + 1..];

        if decimal_part.len() >= 4 {
            // Truncate to 4 decimal places
            format!("{}.{}", integer_part, &decimal_part[..4])
        } else {
            // Pad with zeros to 4 decimal places
            format!("{}.{:0<4}", integer_part, decimal_part)
        }
    } else {
        // No decimal point, add ".0000"
        format!("{}", value)
    }
}

pub struct TelegramLogger {
    service: TelegramService,
    multichain_network_registry: Arc<MultichainNetworkRegistry>,
    error_thread_id: u64,
    error_log_interval_secs: u64,
    last_log_time: Arc<Mutex<Instant>>,
    muted_opportunities: Arc<Mutex<HashMap<String, Instant>>>,
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
            .send_markdown_message_to_general_channel("🟢 Bot started")
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
            muted_opportunities: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Mute an opportunity for a specified duration in seconds
    pub async fn mute_opportunity(&self, opportunity_key: &str, duration_secs: u64) {
        let mut muted_map = self.muted_opportunities.lock().await;
        let expiry = Instant::now() + Duration::from_secs(duration_secs);
        muted_map.insert(opportunity_key.to_string(), expiry);
        info!(
            "Muted opportunity {} for {} seconds",
            opportunity_key, duration_secs
        );
    }

    /// Start the bot dispatcher to handle callback queries
    pub fn start_bot_handler(logger: Arc<TelegramLogger>) {
        let bot = logger.service.bot().clone();
        let logger_clone = logger.clone();

        tokio::spawn(async move {
            info!("Starting Telegram bot dispatcher for callback queries...");

            let handler = Update::filter_callback_query().endpoint(
                move |bot: Throttle<Bot>, callback: CallbackQuery| {
                    let logger = logger_clone.clone();
                    async move { handle_callback_query(bot, callback, logger).await }
                },
            );

            // Don't enable ctrlc_handler here - handle it at the main process level
            let mut dispatcher = Dispatcher::builder(bot, handler).build();

            info!("Telegram bot dispatcher built, starting dispatch...");
            dispatcher.dispatch().await;
            info!("Telegram bot dispatcher stopped");
        });

        info!("Telegram bot dispatcher task spawned");
    }
}

async fn handle_callback_query(
    bot: Throttle<Bot>,
    callback: CallbackQuery,
    logger: Arc<TelegramLogger>,
) -> ResponseResult<()> {
    if let Some(data) = callback.data {
        info!("Callback data: {}", data);
        // Parse callback data: "mute:{opportunity_key}:{duration_secs}"
        if data.starts_with("mute:") {
            let parts: Vec<&str> = data.split(':').collect();
            if parts.len() == 3 {
                let opportunity_key = parts[1];
                if let Ok(duration_secs) = parts[2].parse::<u64>() {
                    logger
                        .mute_opportunity(opportunity_key, duration_secs)
                        .await;

                    // Send confirmation
                    let duration_text = match duration_secs {
                        60 => "1 minute",
                        900 => "15 minutes",
                        3600 => "1 hour",
                        7200 => "2 hours",
                        _ => "a while",
                    };
                    let answer = format!("🔇 Muted for {}", duration_text);
                    info!("Answer: {}", answer);
                    bot.answer_callback_query(callback.id)
                        .text(&answer)
                        .show_alert(false)
                        .await?;
                }
            }
        }
    }
    Ok(())
}

#[async_trait::async_trait]
impl OpportunityLogger for TelegramLogger {
    async fn log_opportunity(
        &self,
        _opportunity_id: &str,
        opportunity: &Opportunity,
    ) -> Result<()> {
        // Create unique identifier for this opportunity using path hash
        let opportunity_key = opportunity.path.hash();

        // Check if this opportunity is muted
        let mut muted_map = self.muted_opportunities.lock().await;
        let now = Instant::now();

        // Clean up expired mutes
        muted_map.retain(|_, &mut expiry| expiry > now);

        // Check if currently muted
        if let Some(expiry) = muted_map.get(&opportunity_key) {
            if *expiry > now {
                let remaining = expiry.duration_since(now);
                let total_secs = remaining.as_secs();
                let remaining_mins = total_secs / 60;
                let remaining_secs = total_secs % 60;
                info!(
                    "Opportunity is muted for {}m {}s, skipping logging",
                    remaining_mins, remaining_secs
                );
                return Ok(()); // Opportunity is muted, skip logging
            }
        }

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
            "💰 *{}*: _{}_ -> _{}_\nProfit: _{}_$\n_{}_ in: *{}*\n_{}_: *{}*\n_{}_ out: *{}*",
            anchor_token_symbol,
            source_chain_name.to_uppercase(),
            target_chain_name.to_uppercase(),
            opportunity.profit,
            source_token_symbol,
            format_decimal_4(&amount_in),
            anchor_token_symbol,
            format_decimal_4(&anchor_token_amount),
            target_token_symbol,
            format_decimal_4(&amount_out)
        );

        // Create inline keyboard with mute buttons
        let keyboard = InlineKeyboardMarkup::new(vec![vec![
            InlineKeyboardButton::callback("🔇 1m", format!("mute:{}:60", opportunity_key)),
            InlineKeyboardButton::callback("🔇 15m", format!("mute:{}:900", opportunity_key)),
            InlineKeyboardButton::callback("🔇 1h", format!("mute:{}:3600", opportunity_key)),
            InlineKeyboardButton::callback("🔇 2h", format!("mute:{}:7200", opportunity_key)),
        ]]);

        self.service
            .send_message_with_keyboard(
                &message,
                keyboard,
                Some(teloxide::types::ParseMode::MarkdownV2),
                None,
            )
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
impl NotificationLogger for TelegramLogger {
    async fn log_error(&self, error: &str) -> Result<()> {
        info!("Logging error: {}", error);
        self.service
            .send_markdown_message_to_thread(&format!("🔴 _{}_", error), self.error_thread_id)
            .await
    }

    async fn log_error_with_chain_id(&self, chain_id: u64, error: &str) -> Result<()> {
        let skip_errors = vec!["invalid block range params", "after last accepted block"];
        if skip_errors.iter().any(|skip| error.contains(skip)) {
            return Ok(());
        }
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

    async fn log_notification(&self, notification: &str) -> Result<()> {
        self.service
            .send_markdown_message_to_thread(notification, self.error_thread_id)
            .await
    }
}
