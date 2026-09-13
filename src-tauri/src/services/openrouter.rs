//! Shared HTTP plumbing for OpenRouter calls (categorisation, insights, Ask).
//!
//! `reqwest::Client::new()` has no timeout, so a request queued behind a busy
//! provider just waited — the button that started it stayed stuck on its
//! spinner with no way out. Every OpenRouter call builds its client here.

use std::time::Duration;

pub const TIMEOUT: Duration = Duration::from_secs(120);

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// Turn a failed send into a message for the UI, naming the likely cause when
/// it was our timeout that fired rather than a network error.
pub fn send_error(e: reqwest::Error) -> String {
    if e.is_timeout() {
        format!(
            "The AI provider didn't respond within {}s — this is usually OpenRouter \
             queueing behind a busy provider. Try again shortly.",
            TIMEOUT.as_secs()
        )
    } else {
        format!("API request failed: {}", e)
    }
}
