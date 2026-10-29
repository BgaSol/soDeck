//! Bounded latest-state delivery. Domain state and transport stay with their owners.
mod feed;
pub mod binary;
pub use feed::{Delivery, Hub, Key, Snapshot, Stats};
pub const STATE_SCHEMA: &str = include_str!("state-feed.generated.json");

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fault(pub &'static str);
pub type Result<T> = std::result::Result<T, Fault>;
