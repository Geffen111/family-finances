//! Shared HTTP plumbing for OpenRouter calls (categorisation, insights, Ask).
//!
//! `reqwest::Client::new()` has no timeout, so a request queued behind a busy
//! provider just waited — the button that started it stayed stuck on its
//! spinner with no way out. Every OpenRouter call builds its client here.
//!
//! The model is a setting, not a constant: `model()` is what every AI feature
//! sends, and `fallback_model()` is tried once when that call fails (a provider
//! error, a timeout, or a reply we cannot parse). `chat_parsed` owns that
//! two-attempt flow so all three features behave the same way.

use std::time::Duration;

pub const TIMEOUT: Duration = Duration::from_secs(120);

/// Used when the user hasn't picked a model — cheap, fast, and good enough at
/// categorising bank lines.
pub const DEFAULT_MODEL: &str = "deepseek/deepseek-v4-flash";

const CHAT_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
const MODELS_URL: &str = "https://openrouter.ai/api/v1/models";

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// One client for the whole process, so the concurrent categorisation batches
/// share a connection pool instead of each building a client of its own.
fn shared_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(client)
}

/// The model to send, from Settings, falling back to [`DEFAULT_MODEL`].
pub fn model() -> String {
    crate::commands::settings::read_str("ai_model").unwrap_or_else(|| DEFAULT_MODEL.to_string())
}

/// The second model to try if the first one fails. `None` when unset or when
/// it is the same id as the primary (retrying the same model buys nothing).
pub fn fallback_model() -> Option<String> {
    crate::commands::settings::read_str("ai_fallback_model").filter(|m| *m != model())
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

// ------------------------------------------------------------- model listing

/// One row of OpenRouter's catalogue, trimmed to what the picker shows.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub context_length: Option<u64>,
    /// USD per million prompt / completion tokens, so the picker can show a
    /// price without the caller doing the arithmetic.
    pub prompt_price: Option<f64>,
    pub completion_price: Option<f64>,
}

#[derive(serde::Deserialize)]
struct ModelsEnvelope {
    data: Vec<RawModel>,
}

#[derive(serde::Deserialize)]
struct RawModel {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    context_length: Option<u64>,
    #[serde(default)]
    pricing: Option<RawPricing>,
}

#[derive(serde::Deserialize)]
struct RawPricing {
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    completion: Option<String>,
}

/// Prices come back as per-token decimal strings; show them per million
/// tokens, which is how providers quote them.
fn per_million(price: &Option<String>) -> Option<f64> {
    price
        .as_deref()
        .and_then(|p| p.parse::<f64>().ok())
        .map(|p| p * 1_000_000.0)
}

/// Fetch OpenRouter's model catalogue. No API key needed — the endpoint is
/// public — so the picker still works before a key is saved.
pub async fn list_models() -> Result<Vec<ModelInfo>, String> {
    let response = shared_client()
        .get(MODELS_URL)
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(send_error)?;

    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| format!("Failed to read model list: {}", e))?;

    if !status.is_success() {
        return Err(format!("OpenRouter model list error ({}): {}", status, text));
    }

    let envelope: ModelsEnvelope = serde_json::from_str(&text)
        .map_err(|e| format!("Failed to parse the model list: {}", e))?;

    let mut models: Vec<ModelInfo> = envelope
        .data
        .into_iter()
        .map(|m| ModelInfo {
            name: m.name.unwrap_or_else(|| m.id.clone()),
            id: m.id,
            context_length: m.context_length,
            prompt_price: per_million(&m.pricing.as_ref().and_then(|p| p.prompt.clone())),
            completion_price: per_million(&m.pricing.as_ref().and_then(|p| p.completion.clone())),
        })
        .collect();
    models.sort_by(|a, b| a.id.to_lowercase().cmp(&b.id.to_lowercase()));
    Ok(models)
}

// ---------------------------------------------------------------- chat calls

/// One chat completion against a named model, returning the message content.
pub async fn chat_once(
    api_key: &str,
    model: &str,
    prompt: &str,
    temperature: f64,
    max_tokens: u32,
) -> Result<String, String> {
    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "temperature": temperature,
        "max_tokens": max_tokens
    });

    let response = shared_client()
        .post(CHAT_URL)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(send_error)?;

    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    if !status.is_success() {
        return Err(format!("OpenRouter API error ({}): {}", status, text));
    }

    #[derive(serde::Deserialize)]
    struct Resp {
        choices: Vec<Choice>,
    }
    #[derive(serde::Deserialize)]
    struct Choice {
        message: Msg,
    }
    #[derive(serde::Deserialize)]
    struct Msg {
        content: String,
    }

    let resp: Resp = serde_json::from_str(&text)
        .map_err(|e| format!("Failed to parse API response: {} - Response: {}", e, text))?;

    resp.choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .ok_or_else(|| "No choices in API response".to_string())
}

/// Strip the code fences models like to wrap JSON replies in.
pub fn strip_code_fences(content: &str) -> &str {
    content
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
}

/// Which model produced an answer, so the UI can say so after a run.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Answer<T> {
    pub value: T,
    pub model: String,
    /// Set when the primary model failed and the fallback answered instead.
    pub fallback_used: bool,
}

/// Call the configured model and parse its reply; on any failure — provider
/// error, timeout, or a reply `parse` rejects — try the fallback model once.
///
/// `parse` is given the raw message content (fences already stripped) so a
/// model that replies with prose where JSON was asked for counts as a failure
/// and hands over to the fallback, rather than failing the whole run.
pub async fn chat_parsed<T, F>(
    api_key: &str,
    prompt: &str,
    temperature: f64,
    max_tokens: u32,
    parse: F,
) -> Result<Answer<T>, String>
where
    F: Fn(&str) -> Result<T, String>,
{
    let primary = model();

    match chat_once(api_key, &primary, prompt, temperature, max_tokens)
        .await
        .and_then(|content| parse(strip_code_fences(&content)))
    {
        Ok(value) => Ok(Answer {
            value,
            model: primary,
            fallback_used: false,
        }),
        Err(first_err) => match fallback_model() {
            None => Err(first_err),
            Some(fb) => match chat_once(api_key, &fb, prompt, temperature, max_tokens)
                .await
                .and_then(|content| parse(strip_code_fences(&content)))
            {
                Ok(value) => Ok(Answer {
                    value,
                    model: fb,
                    fallback_used: true,
                }),
                Err(second_err) => Err(format!(
                    "{} failed ({}), and the fallback {} also failed ({})",
                    primary, first_err, fb, second_err
                )),
            },
        },
    }
}

/// `chat_parsed` for plain-text answers, where any non-empty reply will do.
pub async fn chat_text(
    api_key: &str,
    prompt: &str,
    temperature: f64,
    max_tokens: u32,
) -> Result<Answer<String>, String> {
    chat_parsed(api_key, prompt, temperature, max_tokens, |c| {
        let c = c.trim();
        if c.is_empty() {
            Err("the model returned an empty answer".to_string())
        } else {
            Ok(c.to_string())
        }
    })
    .await
}
