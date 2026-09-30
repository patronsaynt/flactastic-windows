//! Pairing-code lifetime and lockout policy (`PairingGatekeeper.swift`).
//! Expiry is evaluated lazily against the injected clock; the owner schedules
//! a timer to call `close_pairing` so the listener drops the pairing PSK.

use std::time::{Duration, Instant};

use crate::crypto;
use crate::protocol::{PAIRING_CODE_DIGITS, PAIRING_CODE_LIFETIME, PAIRING_FAILURE_LIMIT, PAIRING_LOCKOUT_DURATION};

pub struct PairingGatekeeper {
    active_code: Option<String>,
    code_expires_at: Option<Instant>,
    locked_out_until: Option<Instant>,
    consecutive_failures: u32,
    now: Box<dyn Fn() -> Instant + Send + Sync>,
}

impl Default for PairingGatekeeper {
    fn default() -> Self {
        Self::new(Box::new(Instant::now))
    }
}

impl PairingGatekeeper {
    pub fn new(now: Box<dyn Fn() -> Instant + Send + Sync>) -> Self {
        PairingGatekeeper { active_code: None, code_expires_at: None, locked_out_until: None, consecutive_failures: 0, now }
    }

    fn is_expired(&self) -> bool {
        self.code_expires_at.is_none_or(|t| (self.now)() >= t)
    }

    pub fn is_pairing_open(&self) -> bool {
        self.active_code.is_some() && !self.is_expired()
    }

    pub fn is_locked_out(&self) -> bool {
        self.locked_out_until.is_some_and(|t| (self.now)() < t)
    }

    /// `None` while locked out.
    pub fn open_pairing(&mut self) -> Option<String> {
        if self.is_locked_out() {
            return None;
        }
        let code = crypto::generate_code(PAIRING_CODE_DIGITS);
        self.active_code = Some(code.clone());
        self.code_expires_at = Some((self.now)() + PAIRING_CODE_LIFETIME);
        Some(code)
    }

    pub fn close_pairing(&mut self) {
        self.active_code = None;
        self.code_expires_at = None;
    }

    /// The code an incoming attempt must match; never an expired one.
    pub fn current_code(&self) -> Option<String> {
        if self.is_locked_out() || self.is_expired() {
            return None;
        }
        self.active_code.clone()
    }

    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.locked_out_until = None;
        self.close_pairing();
    }

    /// Every failure burns the code; three in a row lock out for a minute.
    pub fn record_failure(&mut self) {
        self.close_pairing();
        self.consecutive_failures += 1;
        if self.consecutive_failures >= PAIRING_FAILURE_LIMIT {
            self.locked_out_until = Some((self.now)() + PAIRING_LOCKOUT_DURATION);
        }
    }

    pub fn lockout_seconds_remaining(&self) -> u64 {
        match self.locked_out_until {
            Some(t) => {
                let left = t.saturating_duration_since((self.now)());
                left.as_secs() + u64::from(left.subsec_nanos() > 0)
            }
            None => 0,
        }
    }

    pub fn refresh_lockout(&mut self) {
        if self.locked_out_until.is_some_and(|t| (self.now)() >= t) {
            self.locked_out_until = None;
            self.consecutive_failures = 0;
        }
    }

    pub fn code_time_remaining(&self) -> Option<Duration> {
        let t = self.code_expires_at?;
        Some(t.saturating_duration_since((self.now)()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn clock() -> (Arc<Mutex<Instant>>, PairingGatekeeper) {
        let t = Arc::new(Mutex::new(Instant::now()));
        let t2 = t.clone();
        (t, PairingGatekeeper::new(Box::new(move || *t2.lock().unwrap())))
    }

    #[test]
    fn code_expires() {
        let (t, mut g) = clock();
        let code = g.open_pairing().unwrap();
        assert_eq!(g.current_code(), Some(code));
        *t.lock().unwrap() += PAIRING_CODE_LIFETIME;
        assert_eq!(g.current_code(), None);
        assert!(!g.is_pairing_open());
    }

    #[test]
    fn failures_burn_code_and_lock_out() {
        let (t, mut g) = clock();
        for _ in 0..3 {
            g.open_pairing().unwrap();
            g.record_failure();
            assert!(g.current_code().is_none());
        }
        assert!(g.is_locked_out());
        assert!(g.open_pairing().is_none());
        assert_eq!(g.lockout_seconds_remaining(), 60);
        *t.lock().unwrap() += PAIRING_LOCKOUT_DURATION;
        g.refresh_lockout();
        assert!(g.open_pairing().is_some());
    }

    #[test]
    fn success_resets() {
        let (_t, mut g) = clock();
        g.open_pairing();
        g.record_failure();
        g.open_pairing();
        g.record_success();
        assert!(!g.is_pairing_open());
        g.open_pairing();
        g.record_failure();
        g.open_pairing();
        g.record_failure();
        assert!(!g.is_locked_out(), "counter was reset by the success");
    }
}
