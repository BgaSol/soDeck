//! Trusted accounting registry. I/O observers update atomics; sampling is separate.
mod registry;
mod ring;
mod query;
mod services;
#[path="contract.generated.rs"]
mod contract;
#[path="history.generated.rs"]
mod history_contract;
pub(crate) use query::Queries;
#[cfg(test)]
mod tests;
pub(crate) use registry::{Traffic, Owner, Registration, Sample};

pub(crate) fn origin(session: &ssh2::Session, registration: &Option<Registration>) -> std::result::Result<(), ssh2::Error> {
    session.set_channel_origin(registration.as_ref().and_then(|r|r.counter.as_ref()))
}
