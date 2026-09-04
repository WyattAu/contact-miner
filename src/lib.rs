//! # contact-miner
//!
//! Multi-engine web contact discovery. Finds emails, phones, and
//! WhatsApp/Telegram/Signal handles from public web pages via rotating
//! search engines (Brave, Bing, Yandex) and deep-crawling profile pages
//! (Linktree, Beacons).
//!
//! Design goals:
//! - engine rotation with failure backoff (engines rate-limit hard)
//! - obfuscated email de-optimization (`name [at] domain [dot] com`)
//! - anti-bot artifact rejection (challenge-page vendor emails, JS bundle junk)
//! - channel-safe storage (never overwrites known-good contact data)
//!
//! The Python sidecar [`crate`] ships alongside the binary and does the
//! actual HTTP fetching with UA rotation and caching.

pub mod error;
pub mod http;
pub mod parser;
pub mod proxy;
pub mod store;
pub mod validator;
pub mod verifier;

pub use error::{MinerError, Result};
pub use parser::{ContactInfo, HtmlParser, SearchResult};
pub use store::{Lead, LeadStore};
pub use validator::EmailValidator;
pub use verifier::EmailVerifier;
