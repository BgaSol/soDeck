//! Bounded numeric counters for the SoDeck local libssh2 adapter.
//! The C callback cannot execute user code or see payloads. Counter leases stay
//! alive in the owning session until the corresponding C channel is freed.
use libc::{c_int, c_uint, c_void, size_t};
use std::sync::{Arc, atomic::{AtomicBool, AtomicU64, Ordering}};

const MAX_SLOTS: usize = 8192;

#[derive(Default)]
struct Slot {
    token: AtomicU64,
    sent: AtomicU64,
    received: AtomicU64,
    opened: AtomicBool,
    active: AtomicU64,
    closed: AtomicBool,
    invalid: AtomicBool,
}
struct Pool {
    slots: Box<[Slot]>,
    serial: AtomicU64,
    lost_sent: AtomicU64,
    lost_received: AtomicU64,
    invalid: AtomicBool,
}

/// One fixed-size pool can be shared by all SSH sessions in an application.
#[derive(Clone)]
pub struct ByteMeter(Arc<Pool>);

/// A counter is a lease. Dropping its last owner releases the slot.
#[derive(Clone)]
pub struct ByteCounter(Arc<Lease>);
struct Lease { pool: Arc<Pool>, index: usize, token: u64 }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
/// Counts observed at the channel/socket delivery boundary, never payload size estimates.
pub struct ByteCounts {
    /// Bytes successfully written.
    pub sent: u64,
    /// Bytes successfully read.
    pub received: u64,
    /// Whether a C channel has been allocated with this origin.
    pub opened: bool,
    /// Whether all C channels with this origin have been destroyed.
    pub closed: bool,
    /// An unsigned counter saturated; values must be presented as invalid.
    pub invalid: bool,
}

fn add(value: &AtomicU64, count: u64, invalid: &AtomicBool) {
    let _ = value.try_update(Ordering::Relaxed, Ordering::Relaxed, |old| {
        Some(match old.checked_add(count) {
            Some(n) => n,
            None => { invalid.store(true, Ordering::Relaxed); u64::MAX }
        })
    });
}

impl ByteMeter {
    /// Allocate at most 8192 fixed counter slots; no hot-path allocation occurs.
    pub fn new(capacity: usize) -> Self {
        Self(Arc::new(Pool {
            slots: (0..capacity.min(MAX_SLOTS)).map(|_| Slot::default()).collect(),
            serial: AtomicU64::new(1), lost_sent: AtomicU64::new(0),
            lost_received: AtomicU64::new(0), invalid: AtomicBool::new(false),
        }))
    }

    /// Reserve one counter or return None without increasing pool capacity.
    pub fn reserve(&self) -> Option<ByteCounter> {
        let serial = self.0.serial.try_update(Ordering::Relaxed, Ordering::Relaxed,
            |n| if n < (u64::MAX >> 16) { Some(n + 1) } else { None }).ok()?;
        for (index, slot) in self.0.slots.iter().enumerate() {
            // MAX is a reservation sentinel. A published token is never reused.
            if slot.token.compare_exchange(0, u64::MAX, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
                slot.sent.store(0, Ordering::Relaxed);
                slot.received.store(0, Ordering::Relaxed);
                slot.opened.store(false, Ordering::Relaxed);
                slot.active.store(0, Ordering::Relaxed);
                slot.closed.store(false, Ordering::Relaxed);
                slot.invalid.store(false, Ordering::Relaxed);
                let token = (serial << 16) | (index as u64 + 1);
                slot.token.store(token, Ordering::Release);
                return Some(ByteCounter(Arc::new(Lease { pool: self.0.clone(), index, token })));
            }
        }
        None
    }

    /// Unattributed channel bytes remain visible when a caller runs out of slots.
    pub fn unattributed(&self) -> ByteCounts {
        ByteCounts { sent: self.0.lost_sent.load(Ordering::Relaxed),
            received: self.0.lost_received.load(Ordering::Relaxed),
            invalid: self.0.invalid.load(Ordering::Relaxed), ..ByteCounts::default() }
    }

    pub(crate) fn owns(&self, counter: &ByteCounter) -> bool {
        Arc::ptr_eq(&self.0, &counter.0.pool)
    }
}

impl ByteCounter {
    /// Generation-qualified token, unique within this meter's lifetime.
    pub fn token(&self) -> u64 { self.0.token }
    /// Read cumulative values; a common sampler must define snapshot coherence.
    pub fn counts(&self) -> ByteCounts {
        let s = &self.0.pool.slots[self.0.index];
        ByteCounts { sent: s.sent.load(Ordering::Relaxed), received: s.received.load(Ordering::Relaxed),
            opened: s.opened.load(Ordering::Relaxed), closed: s.closed.load(Ordering::Acquire),
            invalid: s.invalid.load(Ordering::Relaxed) }
    }
    /// Record bytes handled by an outer transport protocol that shares this
    /// connection's transport counter (for example SOCKS5 negotiation).
    pub fn record_sent(&self, count: u64) {
        let s = &self.0.pool.slots[self.0.index];
        if s.token.load(Ordering::Acquire) == self.0.token { add(&s.sent, count, &s.invalid); }
    }
    /// Record bytes received by an outer transport protocol.
    pub fn record_received(&self, count: u64) {
        let s = &self.0.pool.slots[self.0.index];
        if s.token.load(Ordering::Acquire) == self.0.token { add(&s.received, count, &s.invalid); }
    }
    pub(crate) fn finish(&self) {
        self.0.pool.slots[self.0.index].closed.store(true, Ordering::Release);
    }
}
impl Drop for Lease {
    fn drop(&mut self) { self.pool.slots[self.index].token.store(0, Ordering::Release); }
}

pub(crate) struct Observer {
    pub meter: ByteMeter,
    pub transport: ByteCounter,
}

pub(crate) unsafe extern "C" fn observe(ctx: *mut c_void, token: u64,
    _channel: c_uint, event: c_int, count: size_t)
{
    // Box<Observer> is pinned by SessionInner through libssh2_session_free.
    // No indexing panic, allocations, locks, user callbacks, or payload access.
    if ctx.is_null() { return; }
    let observer = &*(ctx as *const Observer);
    let pool = &observer.meter.0;
    let token = if event >= 5 { observer.transport.token() } else { token };
    if let Some(index) = (token as usize & 0xffff).checked_sub(1) {
        if let Some(slot) = pool.slots.get(index) {
            if token != 0 && slot.token.load(Ordering::Acquire) == token {
                match event {
                    1 | 5 => add(&slot.sent, count as u64, &slot.invalid),
                    2 | 6 => add(&slot.received, count as u64, &slot.invalid),
                    3 => {
                        let _ = slot.active.fetch_add(1, Ordering::Relaxed);
                        slot.opened.store(true, Ordering::Relaxed);
                        slot.closed.store(false, Ordering::Release);
                    }
                    4 => {
                        if slot.active.fetch_sub(1, Ordering::Relaxed) == 1 {
                            slot.closed.store(true, Ordering::Release);
                        }
                    }
                    _ => {}
                }
                return;
            }
        }
    }
    match event {
        1 => add(&pool.lost_sent, count as u64, &pool.invalid),
        2 => add(&pool.lost_received, count as u64, &pool.invalid),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn leases_are_bounded_and_counters_never_wrap() {
        let meter = ByteMeter::new(2);
        let transport = meter.reserve().unwrap();
        let channel = meter.reserve().unwrap();
        assert!(meter.reserve().is_none());
        let mut observer = Observer { meter: meter.clone(), transport: transport.clone() };
        let ctx = &mut observer as *mut Observer as *mut c_void;
        unsafe {
            observe(ctx, channel.token(), 1, 1, 17);
            observe(ctx, channel.token(), 1, 2, 29);
            observe(ctx, 0, 0, 5, 48);
            observe(ctx, 0, 0, 2, 3);
        }
        assert_eq!((channel.counts().sent, channel.counts().received), (17, 29));
        assert_eq!(transport.counts().sent, 48);
        assert_eq!(meter.unattributed().received, 3);
        let token = channel.token();
        drop(channel);
        let replacement = meter.reserve().unwrap();
        assert_ne!(replacement.token(), token);
        unsafe { observe(ctx, token, 1, 1, 99); }
        assert_eq!(replacement.counts().sent, 0);
        assert_eq!(meter.unattributed().sent, 99);
        let slot = &meter.0.slots[replacement.0.index];
        slot.sent.store(u64::MAX - 1, Ordering::Relaxed);
        unsafe { observe(ctx, replacement.token(), 1, 1, 10); }
        assert_eq!(replacement.counts().sent, u64::MAX);
        assert!(replacement.counts().invalid);
    }
}
