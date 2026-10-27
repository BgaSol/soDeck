//! Bounded latest-state delivery. Domain state and transport stay with their owners.
mod feed;
pub mod binary;
pub use feed::{Delivery, Hub, Key, Snapshot, Stats};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fault(pub &'static str);
pub type Result<T> = std::result::Result<T, Fault>;
