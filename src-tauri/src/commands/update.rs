// Update check. build-info.json is a GitHub *release asset*, which 302-redirects
// to release-assets.githubusercontent.com — a host that sends no CORS header, so
// a fetch() from the webview is silently blocked. We fetch it from Rust (no CORS)
// and hand the parsed marker back to the frontend.

/// The latest published build's marker ({ commit, version, builtAt }), or None if
/// it can't be reached (offline, no release yet, etc.). The frontend compares the
/// commit against the one baked into the running build.
#[tauri::command]
pub async fn latest_build_info(owner: String, repo: String) -> Result<Option<serde_json::Value>, String> {
    let url = format!(
        "https://github.com/{}/{}/releases/download/latest/build-info.json",
        owner, repo
    );
    let client = reqwest::Client::new();
    let resp = match client.get(&url).send().await {
        Ok(r) => r,
        Err(_) => return Ok(None),
    };
    if !resp.status().is_success() {
        return Ok(None);
    }
    let text = match resp.text().await {
        Ok(t) => t,
        Err(_) => return Ok(None),
    };
    // Tolerate the UTF-8 BOM that PowerShell's Set-Content writes.
    let text = text.trim_start_matches('\u{feff}');
    Ok(serde_json::from_str::<serde_json::Value>(text).ok())
}
