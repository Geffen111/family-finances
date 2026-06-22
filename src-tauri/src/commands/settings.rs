use std::path::PathBuf;
use std::fs;

fn settings_path() -> PathBuf {
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("family-finances").join("settings.json")
}

#[tauri::command]
pub async fn save_api_key(key: String) -> Result<(), String> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create settings dir: {}", e))?;
    }
    let settings = serde_json::json!({ "openrouter_api_key": key });
    let json = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;
    fs::write(&path, json).map_err(|e| format!("Failed to write settings: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn get_api_key() -> Result<Option<String>, String> {
    let path = settings_path();
    if !path.exists() {
        return Ok(None);
    }
    let json = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read settings: {}", e))?;
    let settings: serde_json::Value = serde_json::from_str(&json)
        .map_err(|e| format!("Failed to parse settings: {}", e))?;
    Ok(settings.get("openrouter_api_key").and_then(|v| v.as_str().map(|s| s.to_string())))
}
