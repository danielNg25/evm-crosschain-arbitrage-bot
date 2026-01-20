use anyhow::Result;
use log::{error, info};
use teloxide::{
    adaptors::throttle::{Limits, Throttle},
    prelude::*,
    sugar::request::RequestLinkPreviewExt,
    types::{InlineKeyboardMarkup, MessageId, ParseMode, ThreadId},
    Bot,
};

/// Telegram service for sending notifications
///
/// This service provides a clean interface for sending Telegram messages
/// with automatic MarkdownV2 escaping and throttling support.
#[derive(Debug, Clone)]
pub struct TelegramService {
    bot: Throttle<Bot>,
    chat_id: String,
}

impl TelegramService {
    /// Create a new Telegram service
    ///
    /// # Arguments
    /// * `token` - Telegram bot token
    /// * `chat_id` - Chat ID where messages will be sent
    pub fn new(token: String, chat_id: String) -> Self {
        let bot = Bot::new(token).throttle(Limits::default());
        Self { bot, chat_id }
    }

    /// Create a new Telegram service from configuration
    ///
    /// Returns `None` if token or chat_id are not configured
    pub fn from_config(token: Option<String>, chat_id: Option<String>) -> Option<Self> {
        match (token, chat_id) {
            (Some(token), Some(chat_id)) if !token.is_empty() && !chat_id.is_empty() => {
                Some(Self::new(token, chat_id))
            }
            _ => None,
        }
    }

    /// Check if the service is properly configured
    pub fn is_configured(&self) -> bool {
        !self.chat_id.is_empty()
    }

    /// Send a general message to the chat
    ///
    /// # Arguments
    /// * `message` - The message text to send
    /// * `parse_mode` - Optional parse mode (defaults to MarkdownV2)
    /// * `thread_id` - Optional thread ID for forum topics
    /// * `disable_link_preview` - Whether to disable link previews (defaults to true)
    pub async fn send_message(
        &self,
        message: &str,
        parse_mode: Option<ParseMode>,
        thread_id: Option<u64>,
        disable_link_preview: Option<bool>,
    ) -> Result<()> {
        let escaped_message = escape_markdownv2(message);
        let parse_mode = parse_mode.unwrap_or(ParseMode::MarkdownV2);
        let disable_link_preview = disable_link_preview.unwrap_or(true);

        let mut request = self
            .bot
            .send_message(self.chat_id.clone(), escaped_message)
            .disable_link_preview(disable_link_preview)
            .parse_mode(parse_mode);

        if let Some(thread_id) = thread_id {
            request = request.message_thread_id(ThreadId(MessageId(thread_id as i32)));
        }

        request.send().await.map_err(|e| {
            let error_msg = format!("Failed to send Telegram message: {}", e);
            error!("{}", error_msg);
            anyhow::anyhow!(error_msg)
        })?;

        info!("Telegram message sent successfully");
        Ok(())
    }

    /// Send a simple text message (auto-escaped for MarkdownV2)
    ///
    /// This is a convenience method that uses default settings
    pub async fn send_simple_message(&self, message: &str) -> Result<()> {
        self.send_message(message, None, None, Some(true)).await
    }

    /// Send a message to a specific thread
    ///
    /// Convenience method for forum topics
    pub async fn send_to_thread(&self, message: &str, thread_id: u64) -> Result<()> {
        self.send_message(message, None, Some(thread_id), None)
            .await
    }

    pub async fn send_markdown_message_to_general_channel(&self, message: &str) -> Result<()> {
        self.send_message(message, Some(ParseMode::MarkdownV2), None, Some(true))
            .await
    }

    pub async fn send_markdown_message_to_thread(
        &self,
        message: &str,
        thread_id: u64,
    ) -> Result<()> {
        self.send_message(
            message,
            Some(ParseMode::MarkdownV2),
            Some(thread_id),
            Some(true),
        )
        .await
    }

    /// Send a message with inline keyboard buttons
    pub async fn send_message_with_keyboard(
        &self,
        message: &str,
        keyboard: InlineKeyboardMarkup,
        parse_mode: Option<ParseMode>,
        thread_id: Option<u64>,
    ) -> Result<()> {
        let escaped_message = escape_markdownv2(message);
        let parse_mode = parse_mode.unwrap_or(ParseMode::MarkdownV2);

        let mut request = self
            .bot
            .send_message(self.chat_id.clone(), escaped_message)
            .disable_link_preview(true)
            .parse_mode(parse_mode)
            .reply_markup(keyboard);

        if let Some(thread_id) = thread_id {
            request = request.message_thread_id(ThreadId(MessageId(thread_id as i32)));
        }

        request.send().await.map_err(|e| {
            let error_msg = format!("Failed to send Telegram message with keyboard: {}", e);
            error!("{}", error_msg);
            anyhow::anyhow!(error_msg)
        })?;

        info!("Telegram message with keyboard sent successfully");
        Ok(())
    }

    /// Get the bot instance (for advanced operations)
    pub fn bot(&self) -> &Throttle<Bot> {
        &self.bot
    }

    /// Get the chat ID
    pub fn chat_id(&self) -> &str {
        &self.chat_id
    }
}

/// Escape special characters for MarkdownV2
///
/// Telegram's MarkdownV2 requires escaping of special characters:
/// `_`, `[`, `]`, `(`, `)`, `~`, `` ` ``, `>`, `#`, `+`, `-`, `=`, `|`, `{`, `}`, `.`, `!`
pub fn escape_markdownv2(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '[' | ']' | '(' | ')' | '~' | '`' | '>' | '#' | '+' | '-' | '=' | '|' | '{' | '}'
            | '.' | '!' => format!("\\{}", c),
            _ => c.to_string(),
        })
        .collect()
}

/// Escape text for HTML parse mode
///
/// Escapes HTML special characters: `<`, `>`, `&`
pub fn escape_html(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '&' => "&amp;".to_string(),
            _ => c.to_string(),
        })
        .collect()
}
