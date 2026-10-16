use super::model::HistoryPoint;
use std::collections::VecDeque;

// Bound both points and estimated heap storage (large interface counts included).
const MAX_BYTES: usize = 8 * 1024 * 1024;
#[derive(Default)]
pub struct History {
    points: VecDeque<(HistoryPoint, usize)>,
    bytes: usize,
    pub truncated: bool,
}
impl History {
    pub fn push(&mut self, point: HistoryPoint) {
        let bytes = 128 + point.network.keys().map(|k| k.len() + 128).sum::<usize>();
        let cutoff = point.at.saturating_sub(3_600_000);
        self.bytes += bytes;
        self.points.push_back((point, bytes));
        while self.points.front().is_some_and(|(p, _)| p.at < cutoff)
            || self.points.len() > 1801
            || self.bytes > MAX_BYTES
        {
            if self.bytes > MAX_BYTES {
                self.truncated = true;
            }
            if let Some((_, bytes)) = self.points.pop_front() {
                self.bytes -= bytes;
            }
        }
    }
    pub fn since(&self, since: u64) -> Vec<HistoryPoint> {
        self.points
            .iter()
            .filter(|(p, _)| p.at >= since)
            .map(|(p, _)| p.clone())
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keeps_a_bounded_hour_and_real_gaps() {
        let mut h = History::default();
        for sequence in 0..2000 {
            h.push(HistoryPoint {
                sequence,
                at: sequence * 2000,
                cpu: None,
                memory: None,
                network: Default::default(),
                gap: sequence == 1999,
            });
        }
        let points = h.since(0);
        assert_eq!(points.len(), 1801);
        assert!(points.last().unwrap().gap);
        assert_eq!(h.since(3_998_000).len(), 1);
    }
}
