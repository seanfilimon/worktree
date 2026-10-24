//! Polling helpers (used instead of fixed sleeps).

use std::time::{Duration, Instant};

/// Call `f` every `interval` until it returns `Ok(T)` or `timeout` elapses.
/// `Err(String)` describes the last observed state and is included in the
/// panic message on timeout.
#[track_caller]
pub fn poll_until<T>(
    what: &str,
    timeout: Duration,
    interval: Duration,
    mut f: impl FnMut() -> Result<T, String>,
) -> T {
    let start = Instant::now();
    let mut attempts = 0u32;
    loop {
        attempts += 1;
        match f() {
            Ok(v) => return v,
            Err(last) => {
                if start.elapsed() >= timeout {
                    panic!("timed out after {timeout:?} ({attempts} attempts) waiting for {what}\nlast observation:\n{last}");
                }
            }
        }
        std::thread::sleep(interval);
    }
}
