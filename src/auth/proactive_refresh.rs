//! Background task that proactively refreshes OAuth tokens before they expire.
//!
//! Without this, tokens are only refreshed lazily when a request arrives.  If no
//! request arrives during the ~5-minute pre-expiry window the refresh token
//! becomes invalid (Anthropic rotates on use), requiring manual re-auth.
//!
//! Also syncs Anthropic tokens from Claude Code's credential file
//! (`~/.claude/.credentials.json`).  Claude Code's OAuth flow produces tokens
//! with full model access (Sonnet, Opus) because it uses a localhost redirect_uri.
//! CCM cannot replicate this flow when accessed remotely, so it reads Claude
//! Code's token instead.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::auth::{OAuthClient, OAuthConfig, TokenStore};
use crate::auth::token_store::OAuthToken;
use crate::providers::{AuthType, ProviderConfig};
use chrono::{TimeZone, Utc};
use secrecy::{ExposeSecret, SecretString};

/// Map a provider type string to the matching `OAuthConfig`.
pub fn oauth_config_for_provider_type(provider_type: &str) -> Option<OAuthConfig> {
    match provider_type {
        "anthropic" => Some(OAuthConfig::anthropic()),
        "openai" => Some(OAuthConfig::openai_codex()),
        "gemini" => Some(OAuthConfig::gemini()),
        _ => None,
    }
}

/// Scan provider configs, filter to OAuth providers, and build a map from
/// `oauth_provider_id → OAuthConfig`.  API-key providers are skipped.
pub fn build_oauth_provider_map(providers: &[ProviderConfig]) -> HashMap<String, OAuthConfig> {
    let mut map = HashMap::new();
    for p in providers {
        if p.auth_type != AuthType::OAuth {
            continue;
        }
        let Some(ref oauth_id) = p.oauth_provider else {
            continue;
        };
        if map.contains_key(oauth_id) {
            continue; // already mapped
        }
        if let Some(cfg) = oauth_config_for_provider_type(&p.provider_type) {
            map.insert(oauth_id.clone(), cfg);
        } else {
            tracing::debug!(
                "[proactive] skipping unknown provider type '{}' for oauth_provider '{}'",
                p.provider_type,
                oauth_id
            );
        }
    }
    map
}

/// Refresh a single provider's token if it is within the 5-minute expiry window.
///
/// Uses double-checked locking (same pattern as `get_auth_header()` in
/// `anthropic_compatible.rs`) so that concurrent callers (including
/// request-driven refreshes) don't race.
async fn refresh_if_needed(
    provider_id: &str,
    oauth_config: &OAuthConfig,
    token_store: &TokenStore,
) {
    // Fast path: no token, or token is still fresh.
    let token = match token_store.get(provider_id) {
        Some(t) => t,
        None => {
            tracing::debug!("[proactive] no token stored for '{}'", provider_id);
            return;
        }
    };
    if !token.needs_refresh() {
        return;
    }

    // Slow path: acquire per-provider lock, re-check, refresh.
    let lock = token_store.get_refresh_lock(provider_id);
    let _guard = lock.lock().await;

    // Re-read after acquiring the lock – another task may have refreshed already.
    let token = match token_store.get(provider_id) {
        Some(t) => t,
        None => return,
    };
    if !token.needs_refresh() {
        tracing::debug!(
            "[proactive] token for '{}' was refreshed by another task while waiting for lock",
            provider_id
        );
        return;
    }

    tracing::info!(
        "[proactive] refreshing token for '{}' (expires_at: {})",
        provider_id,
        token.expires_at
    );

    let client = OAuthClient::new(oauth_config.clone(), token_store.clone());
    match client.refresh_token_with(provider_id, token).await {
        Ok(new_token) => {
            tracing::info!(
                "[proactive] token refreshed for '{}' (new expires_at: {})",
                provider_id,
                new_token.expires_at
            );
        }
        Err(e) => {
            // `refresh_token_with` already handles `invalid_grant` by removing the
            // dead token, so we just log here.
            tracing::error!("[proactive] failed to refresh token for '{}': {}", provider_id, e);
        }
    }
}

/// Path to Claude Code's credential file.
fn claude_code_credentials_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join(".credentials.json"))
}

/// Sync a single Anthropic OAuth provider's token from Claude Code's credentials.
///
/// Claude Code's OAuth flow uses a localhost redirect_uri which produces tokens
/// with full model access (Sonnet/Opus on Max plans).  CCM's remote admin UI
/// cannot use localhost redirects, so its own OAuth tokens only get Haiku access.
///
/// This function reads Claude Code's token and updates CCM's token store if the
/// Claude Code token is newer or if CCM's token is about to expire.
fn sync_from_claude_code(provider_id: &str, token_store: &TokenStore) {
    let creds_path = match claude_code_credentials_path() {
        Some(p) if p.exists() => p,
        _ => return,
    };

    let content = match std::fs::read_to_string(&creds_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("[proactive] cannot read Claude Code credentials: {}", e);
            return;
        }
    };

    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!("[proactive] cannot parse Claude Code credentials: {}", e);
            return;
        }
    };

    let oauth = match json.get("claudeAiOauth") {
        Some(v) => v,
        None => return,
    };

    let cc_access = match oauth.get("accessToken").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return,
    };
    let cc_refresh = match oauth.get("refreshToken").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return,
    };
    let cc_expires_ms = match oauth.get("expiresAt").and_then(|v| v.as_i64()) {
        Some(ms) => ms,
        None => return,
    };
    let cc_expires_at = Utc.timestamp_millis_opt(cc_expires_ms).single().unwrap_or_else(Utc::now);

    // Skip if Claude Code's token is expired.
    if cc_expires_at <= Utc::now() {
        tracing::debug!("[proactive] Claude Code token is expired, skipping sync");
        return;
    }

    // Check if we need to update: either no CCM token, or CC token is different and fresher.
    if let Some(existing) = token_store.get(provider_id) {
        // Same token – nothing to do.
        if existing.access_token.expose_secret() == cc_access {
            return;
        }
        // CCM token is still fresh and we don't know if CC's is better – only
        // replace if CCM token needs refresh or CC token expires later.
        if !existing.needs_refresh() && cc_expires_at <= existing.expires_at {
            return;
        }
    }

    let token = OAuthToken {
        provider_id: provider_id.to_string(),
        access_token: SecretString::new(cc_access.to_string()),
        refresh_token: SecretString::new(cc_refresh.to_string()),
        expires_at: cc_expires_at,
        enterprise_url: None,
        project_id: None,
    };

    if let Err(e) = token_store.save(token) {
        tracing::warn!("[proactive] failed to save synced Claude Code token: {}", e);
        return;
    }

    tracing::info!(
        "[proactive] synced Claude Code token for '{}' (expires_at: {})",
        provider_id,
        cc_expires_at,
    );
}

/// Spawn the background refresh loop.
///
/// Checks all OAuth tokens every 60 seconds and refreshes any that are within
/// 5 minutes of expiry.  The returned `JoinHandle` can be used to cancel the
/// task on shutdown, but it is safe to simply drop it.
pub fn spawn(
    token_store: TokenStore,
    providers: &[ProviderConfig],
) -> tokio::task::JoinHandle<()> {
    let oauth_map = build_oauth_provider_map(providers);

    if oauth_map.is_empty() {
        tracing::debug!("[proactive] no OAuth providers configured, skipping background refresh");
        return tokio::spawn(async {});
    }

    tracing::info!(
        "[proactive] starting background refresh for {} OAuth provider(s): {}",
        oauth_map.len(),
        oauth_map.keys().cloned().collect::<Vec<_>>().join(", ")
    );

    // Identify which providers are Anthropic (eligible for Claude Code token sync).
    let anthropic_providers: Vec<String> = oauth_map
        .iter()
        .filter(|(_, cfg)| cfg.client_id == "9d1c250a-e61b-44d9-88ed-5944d1962f5e")
        .map(|(id, _)| id.clone())
        .collect();

    // Do an initial sync immediately so CCM picks up Claude Code's token on startup.
    for provider_id in &anthropic_providers {
        sync_from_claude_code(provider_id, &token_store);
    }

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        // The first tick completes immediately – skip it so we don't refresh
        // right at startup (the tokens were just loaded / synced).
        interval.tick().await;

        loop {
            interval.tick().await;

            // Sync from Claude Code first (Anthropic providers only).
            for provider_id in &anthropic_providers {
                sync_from_claude_code(provider_id, &token_store);
            }

            // Proactive refresh for non-Anthropic providers only.
            // Anthropic tokens are managed by Claude Code via sync above –
            // refreshing them here with CCM's OAuthConfig would produce
            // console-type tokens that lack Sonnet/Opus access, or get
            // invalid_grant because CC already rotated the refresh token.
            for (provider_id, oauth_config) in &oauth_map {
                if anthropic_providers.contains(provider_id) {
                    continue;
                }
                refresh_if_needed(provider_id, oauth_config, &token_store).await;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::token_store::OAuthToken;
    use chrono::Utc;
    use secrecy::{ExposeSecret, SecretString};
    use tempfile::TempDir;

    #[test]
    fn test_oauth_config_for_provider_type() {
        assert!(oauth_config_for_provider_type("anthropic").is_some());
        assert!(oauth_config_for_provider_type("openai").is_some());
        assert!(oauth_config_for_provider_type("gemini").is_some());
        assert!(oauth_config_for_provider_type("unknown").is_none());
        assert!(oauth_config_for_provider_type("").is_none());
    }

    #[test]
    fn test_build_oauth_provider_map() {
        let providers = vec![
            ProviderConfig {
                name: "claude-max".to_string(),
                provider_type: "anthropic".to_string(),
                auth_type: AuthType::OAuth,
                api_key: None,
                oauth_provider: Some("my-anthropic".to_string()),
                project_id: None,
                location: None,
                base_url: None,
                headers: None,
                models: vec!["claude-sonnet-4-6".to_string()],
                enabled: Some(true),
            },
            ProviderConfig {
                name: "openai-api".to_string(),
                provider_type: "openai".to_string(),
                auth_type: AuthType::ApiKey,
                api_key: Some("sk-xxx".to_string()),
                oauth_provider: None,
                project_id: None,
                location: None,
                base_url: None,
                headers: None,
                models: vec!["gpt-4".to_string()],
                enabled: Some(true),
            },
        ];

        let map = build_oauth_provider_map(&providers);
        assert_eq!(map.len(), 1);
        assert!(map.contains_key("my-anthropic"));
        // API key provider should not appear
        assert!(!map.values().any(|c| c.client_id.contains("openai")));
    }

    #[tokio::test]
    async fn test_refresh_skips_fresh_token() {
        let temp_dir = TempDir::new().unwrap();
        let token_path = temp_dir.path().join("tokens.json");
        let store = TokenStore::new(token_path).unwrap();

        // Save a token that does NOT need refresh (1 hour from now).
        let token = OAuthToken {
            provider_id: "test-provider".to_string(),
            access_token: SecretString::new("access-fresh".to_string()),
            refresh_token: SecretString::new("refresh-fresh".to_string()),
            expires_at: Utc::now() + chrono::Duration::hours(1),
            enterprise_url: None,
            project_id: None,
        };
        store.save(token).unwrap();

        let config = OAuthConfig::anthropic();

        // This should return without attempting any HTTP call.
        refresh_if_needed("test-provider", &config, &store).await;

        // Verify the token is unchanged (no refresh happened).
        let stored = store.get("test-provider").unwrap();
        assert_eq!(stored.access_token.expose_secret(), "access-fresh");
    }
}
