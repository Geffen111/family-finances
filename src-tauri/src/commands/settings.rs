//! App settings, stored as a small JSON file in AppData (never in the repo —
//! it holds the OpenRouter API key).

use std::fs;
use std::path::PathBuf;

fn settings_path() -> PathBuf {
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("family-finances").join("settings.json")
}

/// The whole settings object, or `{}` when there's no file yet / it's corrupt.
fn load() -> serde_json::Value {
    let path = settings_path();
    if !path.exists() {
        return serde_json::json!({});
    }
    fs::read_to_string(&path)
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

/// Read one string setting, treating an empty string as unset.
pub fn read_str(key: &str) -> Option<String> {
    load()
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Merge one key into the stored settings, leaving the others untouched.
fn write_str(key: &str, value: &str) -> Result<(), String> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create settings dir: {}", e))?;
    }
    let mut settings = load();
    settings[key] = serde_json::json!(value);
    let json = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;
    fs::write(&path, json).map_err(|e| format!("Failed to write settings: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn save_api_key(key: String) -> Result<(), String> {
    write_str("openrouter_api_key", &key)
}

#[tauri::command]
pub async fn get_api_key() -> Result<Option<String>, String> {
    Ok(read_str("openrouter_api_key"))
}

#[tauri::command]
pub async fn save_household_name(name: String) -> Result<(), String> {
    write_str("household_name", &name)
}

#[tauri::command]
pub async fn get_household_name() -> Result<Option<String>, String> {
    Ok(read_str("household_name"))
}

// ---------------------------------------------------------------- AI model

/// The model every AI feature uses (categorisation, Ask, insights), plus an
/// optional fallback tried when the first one errors or replies with something
/// we can't parse. Both are OpenRouter model ids; unset means the built-in
/// default (`openrouter::DEFAULT_MODEL`).
#[derive(Debug, serde::Serialize)]
pub struct AiModelSettings {
    pub model: String,
    pub fallback_model: Option<String>,
    pub default_model: String,
}

#[tauri::command]
pub async fn get_ai_model_settings() -> Result<AiModelSettings, String> {
    Ok(AiModelSettings {
        model: crate::services::openrouter::model(),
        fallback_model: crate::services::openrouter::fallback_model(),
        default_model: crate::services::openrouter::DEFAULT_MODEL.to_string(),
    })
}

/// Save the model choice. An empty `model` resets to the built-in default; an
/// empty/absent `fallback_model` clears the fallback.
#[tauri::command]
pub async fn save_ai_model_settings(
    model: String,
    fallback_model: Option<String>,
) -> Result<AiModelSettings, String> {
    write_str("ai_model", model.trim())?;
    write_str(
        "ai_fallback_model",
        fallback_model.unwrap_or_default().trim(),
    )?;
    get_ai_model_settings().await
}

/// The models OpenRouter currently offers, for the Settings picker.
#[tauri::command]
pub async fn list_ai_models() -> Result<Vec<crate::services::openrouter::ModelInfo>, String> {
    crate::services::openrouter::list_models().await
}

/// Send a one-line prompt to a model to check the id is valid and the key
/// works, reporting how long it took.
#[tauri::command]
pub async fn test_ai_model(model: String) -> Result<String, String> {
    let api_key = read_str("openrouter_api_key").ok_or_else(|| {
        "OpenRouter API key not configured. Save your API key first.".to_string()
    })?;
    let model = model.trim();
    let model = if model.is_empty() {
        crate::services::openrouter::model()
    } else {
        model.to_string()
    };
    let started = std::time::Instant::now();
    let reply =
        crate::services::openrouter::chat_once(&api_key, &model, "Reply with the word: ready", 0.0, 16)
            .await?;
    let ms = started.elapsed().as_millis();
    let reply = reply.trim().chars().take(60).collect::<String>();
    Ok(format!("{} replied in {} ms: \u{201c}{}\u{201d}", model, ms, reply))
}
