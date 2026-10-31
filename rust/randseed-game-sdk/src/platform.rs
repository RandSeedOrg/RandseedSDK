//! Platform client used by RandSeed game servers.
//!
//! Makes asynchronous requests to the room host's `http://platform.*` virtual
//! hosts.

use crate::errors::SdkError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Information about a player currently connected to the room.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlayerInfo {
    /// Player identifier.
    #[serde(rename = "playerId")]
    pub player_id: String,
    /// Connection time in epoch milliseconds.
    #[serde(rename = "joinedAt")]
    pub joined_at: u64,
}

#[derive(Debug, Deserialize)]
struct SqlResponse {
    #[serde(default)]
    rows: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct BroadcastResponse {
    success: bool,
    #[serde(rename = "recipientCount", default)]
    recipient_count: u64,
}

#[derive(Debug, Deserialize)]
struct SendResponse {
    success: bool,
    #[serde(default)]
    delivered: bool,
}

#[derive(Debug, Deserialize)]
struct PlayersResponse {
    #[serde(default)]
    players: Vec<PlayerInfo>,
}

#[derive(Debug, Deserialize)]
struct KvGetResponse {
    value: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct KvMutationResponse {
    success: bool,
}

/// Asynchronous client for room-host platform services.
#[derive(Debug, Clone)]
pub struct PlatformClient {
    client: reqwest::Client,
    vhost_map: HashMap<String, String>,
}

impl Default for PlatformClient {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformClient {
    /// Create a client that targets the default `http://platform.*` virtual hosts.
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            vhost_map: HashMap::new(),
        }
    }

    /// Create a client with custom virtual-host mappings, useful for local testing.
    pub fn with_vhost_map(vhost_map: HashMap<String, String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            vhost_map,
        }
    }

    fn resolve_url(&self, vhost: &str, path: &str) -> String {
        let base = self
            .vhost_map
            .get(vhost)
            .cloned()
            .unwrap_or_else(|| format!("http://{}", vhost));
        let clean_path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{}", path)
        };
        format!("{}{}", base, clean_path)
    }

    /// Execute a low-frequency parameterized SQLite query.
    pub async fn sql(
        &self,
        query: &str,
        params: Vec<serde_json::Value>,
    ) -> Result<Vec<serde_json::Value>, SdkError> {
        let url = self.resolve_url("platform.db", "/sql");
        let payload = serde_json::json!({
            "sql": query,
            "params": params,
        });

        let resp = self.client.post(&url).json(&payload).send().await?;
        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let err_text = resp.text().await.unwrap_or_default();
            return Err(SdkError::PlatformHttp(status, err_text));
        }

        let data: SqlResponse = resp.json().await?;
        Ok(data.rows)
    }

    /// Broadcast a WebSocket message to all online players in the room.
    ///
    /// Returns `(success, recipient_count)`.
    pub async fn broadcast(&self, message: serde_json::Value) -> Result<(bool, u64), SdkError> {
        let url = self.resolve_url("platform.broadcast", "/");
        let payload = serde_json::json!({
            "message": message,
        });

        let resp = self.client.post(&url).json(&payload).send().await?;
        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let err_text = resp.text().await.unwrap_or_default();
            return Err(SdkError::PlatformHttp(status, err_text));
        }

        let data: BroadcastResponse = resp.json().await?;
        Ok((data.success, data.recipient_count))
    }

    /// Send a private WebSocket message to one player.
    ///
    /// Returns `(success, delivered)`.
    pub async fn send(
        &self,
        player_id: &str,
        message: serde_json::Value,
    ) -> Result<(bool, bool), SdkError> {
        if player_id.trim().is_empty() {
            return Err(SdkError::Validation(
                "A valid playerId is required for a direct message.".into(),
            ));
        }

        let url = self.resolve_url("platform.send", "/");
        let payload = serde_json::json!({
            "playerId": player_id,
            "message": message,
        });

        let resp = self.client.post(&url).json(&payload).send().await?;
        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let err_text = resp.text().await.unwrap_or_default();
            return Err(SdkError::PlatformHttp(status, err_text));
        }

        let data: SendResponse = resp.json().await?;
        Ok((data.success, data.delivered))
    }

    /// Return the list of all online players in the room.
    pub async fn get_players(&self) -> Result<Vec<PlayerInfo>, SdkError> {
        let url = self.resolve_url("platform.players", "/");
        let resp = self.client.get(&url).send().await?;
        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let err_text = resp.text().await.unwrap_or_default();
            return Err(SdkError::PlatformHttp(status, err_text));
        }

        let data: PlayersResponse = resp.json().await?;
        Ok(data.players)
    }

    /// Read a temporary key-value entry for the room lifecycle.
    pub async fn kv_get(&self, key: &str) -> Result<Option<serde_json::Value>, SdkError> {
        if key.trim().is_empty() {
            return Err(SdkError::Validation(
                "A valid key is required for the room key-value store.".into(),
            ));
        }

        let path = format!("/{}", urlencoding::encode(key));
        let url = self.resolve_url("platform.kv", &path);
        let resp = self.client.get(&url).send().await?;
        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let err_text = resp.text().await.unwrap_or_default();
            return Err(SdkError::PlatformHttp(status, err_text));
        }

        let data: KvGetResponse = resp.json().await?;
        Ok(data.value)
    }

    /// Write or update a temporary key-value entry for the room lifecycle.
    pub async fn kv_put(&self, key: &str, value: serde_json::Value) -> Result<bool, SdkError> {
        if key.trim().is_empty() {
            return Err(SdkError::Validation(
                "A valid key is required for the room key-value store.".into(),
            ));
        }

        let path = format!("/{}", urlencoding::encode(key));
        let url = self.resolve_url("platform.kv", &path);
        let payload = serde_json::json!({
            "value": value,
        });

        let resp = self.client.put(&url).json(&payload).send().await?;
        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let err_text = resp.text().await.unwrap_or_default();
            return Err(SdkError::PlatformHttp(status, err_text));
        }

        let data: KvMutationResponse = resp.json().await?;
        Ok(data.success)
    }

    /// Delete a temporary key-value entry for the room lifecycle.
    pub async fn kv_delete(&self, key: &str) -> Result<bool, SdkError> {
        if key.trim().is_empty() {
            return Err(SdkError::Validation(
                "A valid key is required for the room key-value store.".into(),
            ));
        }

        let path = format!("/{}", urlencoding::encode(key));
        let url = self.resolve_url("platform.kv", &path);
        let resp = self.client.delete(&url).send().await?;
        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let err_text = resp.text().await.unwrap_or_default();
            return Err(SdkError::PlatformHttp(status, err_text));
        }

        let data: KvMutationResponse = resp.json().await?;
        Ok(data.success)
    }
}
