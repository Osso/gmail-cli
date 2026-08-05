#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod api;
pub mod auth;
pub mod config;
pub mod mime;

pub use api::{Client, Label, LabelList, Message, MessageList, MessageRef};
pub use config::{Config, Tokens};
