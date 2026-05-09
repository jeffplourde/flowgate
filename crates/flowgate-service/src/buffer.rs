use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::time::{Duration, Instant};

use async_nats::HeaderMap;
use bytes::Bytes;

pub struct BufferedMessage {
    pub score: f64,
    pub arrived: Instant,
    pub payload: Bytes,
    pub headers: HeaderMap,
}

impl PartialEq for BufferedMessage {
    fn eq(&self, other: &Self) -> bool {
        self.score == other.score
    }
}

impl Eq for BufferedMessage {}

impl PartialOrd for BufferedMessage {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BufferedMessage {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score
            .partial_cmp(&other.score)
            .unwrap_or(Ordering::Equal)
    }
}

pub struct MessageBuffer {
    heap: BinaryHeap<BufferedMessage>,
    max_duration: Duration,
}

impl MessageBuffer {
    pub fn new(max_duration_ms: u64) -> Self {
        Self {
            heap: BinaryHeap::new(),
            max_duration: Duration::from_millis(max_duration_ms),
        }
    }

    pub fn push(&mut self, msg: BufferedMessage) {
        self.heap.push(msg);
    }

    pub fn len(&self) -> usize {
        self.heap.len()
    }

    pub fn set_max_duration(&mut self, ms: u64) {
        self.max_duration = Duration::from_millis(ms);
    }

    /// Pop the single highest-scoring message. Returns None if buffer is empty.
    pub fn drain_one(&mut self) -> Option<BufferedMessage> {
        self.heap.pop()
    }

    /// Pop the top N highest-scoring messages.
    pub fn drain_top_n(&mut self, n: usize) -> Vec<BufferedMessage> {
        let mut result = Vec::with_capacity(n.min(self.heap.len()));
        for _ in 0..n {
            match self.heap.pop() {
                Some(msg) => result.push(msg),
                None => break,
            }
        }
        result
    }

    /// Remove messages that have been in the buffer longer than max_duration.
    /// Returns the number of evicted messages.
    pub fn evict_expired(&mut self, now: Instant) -> usize {
        let deadline = now - self.max_duration;
        let before = self.heap.len();
        let old_heap = std::mem::take(&mut self.heap);
        self.heap = old_heap
            .into_iter()
            .filter(|msg| msg.arrived >= deadline)
            .collect();
        before - self.heap.len()
    }

    /// Drain for batch mode: evict expired, then return top N from what remains,
    /// dropping the rest.
    pub fn drain_batch(&mut self, n: usize, now: Instant) -> Vec<BufferedMessage> {
        self.evict_expired(now);
        let result = self.drain_top_n(n);
        self.heap.clear();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(score: f64, age_ms: u64) -> BufferedMessage {
        BufferedMessage {
            score,
            arrived: Instant::now() - Duration::from_millis(age_ms),
            payload: Bytes::from_static(b"test"),
            headers: HeaderMap::new(),
        }
    }

    #[test]
    fn drain_one_returns_highest() {
        let mut buf = MessageBuffer::new(5000);
        buf.push(msg(0.3, 0));
        buf.push(msg(0.9, 0));
        buf.push(msg(0.5, 0));

        let best = buf.drain_one().unwrap();
        assert!((best.score - 0.9).abs() < f64::EPSILON);
        assert_eq!(buf.len(), 2);
    }

    #[test]
    fn drain_top_n() {
        let mut buf = MessageBuffer::new(5000);
        for i in 0..10 {
            buf.push(msg(i as f64 / 10.0, 0));
        }

        let top3 = buf.drain_top_n(3);
        assert_eq!(top3.len(), 3);
        assert!((top3[0].score - 0.9).abs() < f64::EPSILON);
        assert!((top3[1].score - 0.8).abs() < f64::EPSILON);
        assert!((top3[2].score - 0.7).abs() < f64::EPSILON);
        assert_eq!(buf.len(), 7);
    }

    #[test]
    fn drain_top_n_more_than_available() {
        let mut buf = MessageBuffer::new(5000);
        buf.push(msg(0.5, 0));
        buf.push(msg(0.7, 0));

        let result = buf.drain_top_n(10);
        assert_eq!(result.len(), 2);
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn evict_expired() {
        let mut buf = MessageBuffer::new(100);
        buf.push(msg(0.9, 200)); // 200ms old, should be evicted
        buf.push(msg(0.5, 50)); // 50ms old, should stay
        buf.push(msg(0.3, 150)); // 150ms old, should be evicted

        let evicted = buf.evict_expired(Instant::now());
        assert_eq!(evicted, 2);
        assert_eq!(buf.len(), 1);
        assert!((buf.drain_one().unwrap().score - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn drain_batch_evicts_then_returns_top_n() {
        let mut buf = MessageBuffer::new(100);
        buf.push(msg(0.9, 200)); // expired
        buf.push(msg(0.8, 50)); // fresh
        buf.push(msg(0.7, 30)); // fresh
        buf.push(msg(0.6, 20)); // fresh
        buf.push(msg(0.1, 10)); // fresh, low score

        let result = buf.drain_batch(2, Instant::now());
        assert_eq!(result.len(), 2);
        assert!((result[0].score - 0.8).abs() < f64::EPSILON);
        assert!((result[1].score - 0.7).abs() < f64::EPSILON);
        assert_eq!(buf.len(), 0); // remaining dropped
    }

    #[test]
    fn empty_buffer() {
        let mut buf = MessageBuffer::new(5000);
        assert!(buf.drain_one().is_none());
        assert_eq!(buf.drain_top_n(5).len(), 0);
        assert_eq!(buf.evict_expired(Instant::now()), 0);
    }
}
