//! Mail provider abstraction. Providers yield a **stream** of raw messages so
//! arbitrarily large mailboxes never materialize in memory.

pub mod gmail;
pub mod imap;

use crate::error::Result;
use crate::model::Filter;
use async_trait::async_trait;
use futures::stream::Stream;
use std::pin::Pin;

/// One raw message pulled from a provider, still in RFC822 form.
#[derive(Debug, Clone)]
pub struct RawMessage {
    /// Provider-native id: Gmail message id, or the IMAP UID rendered as text.
    pub provider_message_id: String,
    /// IMAP UID (0 for Gmail).
    pub uid: u32,
    /// Server internal date in unix seconds (0 if unknown).
    pub internal_date: i64,
    /// Raw RFC822 bytes.
    pub raw: Vec<u8>,
}

pub type MessageStream = Pin<Box<dyn Stream<Item = Result<RawMessage>> + Send>>;

#[async_trait]
pub trait MailProvider: Send + Sync {
    /// Cheap connectivity/credential check used when adding an account.
    async fn test_connection(&self) -> Result<()>;

    /// Stream messages matching `filter`.
    async fn fetch(&self, filter: &Filter) -> Result<MessageStream>;
}
