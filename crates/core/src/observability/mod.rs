//! Public facts and projections share one write owner. Domain modules own their events.
pub(crate) mod schema;
mod writer;
mod calls;
mod query;
mod emergency;
mod quality;
mod events;
mod services;
mod sequence;
mod model;
mod catalog;
mod context;
mod ingest;
mod history;
mod policy;
mod snapshots;
mod metrics;
mod feed;
mod feed_cache;
mod evidence;
mod evidence_scope;
mod automatic;
mod retention;
mod targets;
mod builtin;
pub(crate) mod resource;
mod credits;
pub use feed::ObservationDelivery;
#[path="../../../../contracts/observability_descriptor.rs"]
pub(crate) mod descriptor;
pub(crate) mod traffic;
pub(crate) use writer::Observability;
pub(crate) use calls::AuditScope;
pub use context::DiagnosticScope;

fn integer(row:&rusqlite::Row<'_>,index:usize)->rusqlite::Result<u64> {
    let value:i64=row.get(index)?;value.try_into().map_err(|_|rusqlite::Error::IntegralValueOutOfRange(index,value))
}
fn optional_integer(row:&rusqlite::Row<'_>,index:usize)->rusqlite::Result<Option<u64>> {
    let value:Option<i64>=row.get(index)?;value.map(|v|v.try_into().map_err(|_|rusqlite::Error::IntegralValueOutOfRange(index,v))).transpose()
}

pub use emergency::with_startup_diagnostics_io;
