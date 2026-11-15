//! What a host and its clients tell the time by.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The time as the real clock has it or, for tests, time that passes only
/// when it's told to.
#[derive(Clone, Debug, Default)]
pub struct Clock(Option<Arc<Mutex<Instant>>>);

impl Clock {
    /// A clock that stands still until it's [passed](Clock::pass).
    pub fn stopped() -> Clock {
        Clock(Some(Arc::new(Mutex::new(Instant::now()))))
    }

    /// Moves a [stopped](Clock::stopped) clock, and every copy of it, on
    /// by `time`; the real one goes on by itself.
    pub fn pass(&self, time: Duration) {
        if let Some(now) = &self.0 {
            *now.lock().unwrap_or_else(|e| e.into_inner()) += time;
        }
    }

    pub fn now(&self) -> Instant {
        match &self.0 {
            None => Instant::now(),
            Some(now) => *now.lock().unwrap_or_else(|e| e.into_inner()),
        }
    }
}
