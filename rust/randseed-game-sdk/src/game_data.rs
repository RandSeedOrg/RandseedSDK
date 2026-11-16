//! Typed client and contracts for the versioned game-owned data service.
use crate::errors::SdkError;
use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};

/// Version used by all requests sent to the Room host game-data protocol.
pub const GAME_DATA_PROTOCOL_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
/// Supported top-level scalar types for game-owned JSON documents.
#[allow(missing_docs)]
pub enum GameDataFieldType {
    String,
    Number,
    Boolean,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Declared versioned field and index contract for one collection.
#[allow(missing_docs)]
pub struct GameDataSchema {
    pub version: u32,
    pub fields: BTreeMap<String, GameDataFieldType>,
    #[serde(default)]
    pub indexes: Vec<String>,
    /// Equality-field then sort-field compound access paths; arbitrary compound queries are unsupported.
    #[serde(default)]
    pub compound_indexes: Vec<[String; 2]>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// A live document or revision-bearing deletion tombstone.
#[allow(missing_docs)]
pub struct GameDataRecord {
    pub id: String,
    pub value: Option<Value>,
    pub revision: u64,
    pub deleted: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "lowercase", deny_unknown_fields)]
/// One compare-and-swap put or delete in an atomic write.
#[allow(missing_docs)]
pub enum GameDataMutation {
    Put {
        collection: String,
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: u64,
        value: Value,
    },
    Delete {
        collection: String,
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: u64,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Idempotency key and atomic set of document mutations.
#[allow(missing_docs)]
pub struct GameDataWriteRequest {
    pub idempotency_key: String,
    pub mutations: Vec<GameDataMutation>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// Committed revision for one document mutation.
#[allow(missing_docs)]
pub struct GameDataWriteReceiptItem {
    pub collection: String,
    pub id: String,
    pub revision: u64,
    pub deleted: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// Stable receipt returned for an accepted idempotent write.
#[allow(missing_docs)]
pub struct GameDataWriteReceipt {
    pub idempotency_key: String,
    pub items: Vec<GameDataWriteReceiptItem>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
/// Supported declarative scalar comparison operators.
#[allow(missing_docs)]
pub enum GameDataFilterOperator {
    Eq,
    Lt,
    Lte,
    Gt,
    Gte,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// One comparison against a declared indexed scalar field.
#[allow(missing_docs)]
pub struct GameDataFilter {
    pub field: String,
    pub op: GameDataFilterOperator,
    pub value: Value,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
/// Supported indexed sort directions.
#[allow(missing_docs)]
pub enum GameDataSortDirection {
    Asc,
    Desc,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Ordering by one declared indexed scalar field.
#[allow(missing_docs)]
pub struct GameDataSort {
    pub field: String,
    pub direction: GameDataSortDirection,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Bounded indexed query with optional keyset cursor.
#[allow(missing_docs)]
pub struct GameDataQuery {
    pub collection: String,
    #[serde(default, rename = "where")]
    pub filters: Vec<GameDataFilter>,
    #[serde(default)]
    pub order_by: Option<GameDataSort>,
    pub limit: u16,
    #[serde(default)]
    pub cursor: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// Live records and the next opaque keyset cursor, if any.
#[allow(missing_docs)]
pub struct GameDataQueryResult {
    pub items: Vec<GameDataRecord>,
    pub next_cursor: Option<String>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
/// Supported bounded aggregate operations.
#[allow(missing_docs)]
pub enum GameDataAggregateOperator {
    Count,
    Sum,
    Min,
    Max,
    Avg,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Aggregate operator and optional numeric field.
#[allow(missing_docs)]
pub struct GameDataAggregateMetric {
    pub op: GameDataAggregateOperator,
    #[serde(default)]
    pub field: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Bounded aggregate request with an explicit scan limit.
#[allow(missing_docs)]
pub struct GameDataAggregate {
    pub collection: String,
    #[serde(default, rename = "where")]
    pub filters: Vec<GameDataFilter>,
    pub metric: GameDataAggregateMetric,
    pub scan_limit: u16,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// Aggregate value and number of records scanned.
#[allow(missing_docs)]
pub struct GameDataAggregateResult {
    pub value: Option<f64>,
    pub scanned: u16,
}

fn safe_error_code(code: Option<&str>) -> String {
    code.filter(|value| {
        !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == 95)
    })
    .unwrap_or("GAME_DATA_HTTP_ERROR")
    .to_owned()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResponseEnvelope<T> {
    success: bool,
    data: Option<T>,
    code: Option<String>,
}
#[derive(Debug, Clone)]
/// Asynchronous client for the room-host game-owned data service.
#[allow(missing_docs)]
pub struct GameDataClient {
    client: Client,
    vhost_map: HashMap<String, String>,
}
impl GameDataClient {
    pub(crate) fn new(client: Client, vhost_map: HashMap<String, String>) -> Self {
        Self { client, vhost_map }
    }
    fn url(&self, operation: &str) -> String {
        let base = self
            .vhost_map
            .get("platform.data")
            .cloned()
            .unwrap_or_else(|| "http://platform.data".into());
        format!("{}/{}", base.trim_end_matches("/"), operation)
    }
    async fn call<T: DeserializeOwned>(
        &self,
        operation: &str,
        request: Value,
    ) -> Result<Option<T>, SdkError> {
        let response = self
            .client
            .post(self.url(operation))
            .json(&json!({ "protocolVersion": GAME_DATA_PROTOCOL_VERSION, "request": request }))
            .send()
            .await?;
        let status = response.status().as_u16();
        let payload = response.json::<Value>().await.unwrap_or(Value::Null);
        if !(200..300).contains(&status) {
            let code = safe_error_code(payload.get("code").and_then(Value::as_str));
            return Err(SdkError::PlatformData { status, code });
        }
        let envelope: ResponseEnvelope<T> =
            serde_json::from_value(payload).map_err(|_| SdkError::PlatformData {
                status,
                code: "GAME_DATA_INVALID_RESPONSE".into(),
            })?;
        if !envelope.success {
            return Err(SdkError::PlatformData {
                status,
                code: safe_error_code(envelope.code.as_deref()),
            });
        }
        Ok(envelope.data)
    }
    fn validate_name(name: &str, label: &str) -> Result<(), SdkError> {
        if name.is_empty() || name.trim() != name {
            return Err(SdkError::Validation(format!(
                "A non-empty {} is required.",
                label
            )));
        }
        Ok(())
    }
    /// Register a new schema or an additive, higher-version compatible schema.
    pub async fn register_schema(
        &self,
        collection: &str,
        schema: &GameDataSchema,
    ) -> Result<(), SdkError> {
        Self::validate_name(collection, "collection")?;
        let _: Option<Value> = self
            .call(
                "schema",
                json!({ "collection": collection, "schema": schema }),
            )
            .await?;
        Ok(())
    }
    /// Read a live record or its revision-bearing deletion tombstone.
    pub async fn get(
        &self,
        collection: &str,
        id: &str,
    ) -> Result<Option<GameDataRecord>, SdkError> {
        Self::validate_name(collection, "collection")?;
        Self::validate_name(id, "id")?;
        self.call("get", json!({ "collection": collection, "id": id }))
            .await
    }
    /// Atomically create or replace one record using its expected revision.
    pub async fn put(
        &self,
        idempotency_key: &str,
        collection: &str,
        id: &str,
        expected_revision: u64,
        value: Value,
    ) -> Result<GameDataWriteReceipt, SdkError> {
        self.mutate(&GameDataWriteRequest {
            idempotency_key: idempotency_key.into(),
            mutations: vec![GameDataMutation::Put {
                collection: collection.into(),
                id: id.into(),
                expected_revision,
                value,
            }],
        })
        .await
    }
    /// Delete one record without discarding its monotonically increasing revision.
    pub async fn delete(
        &self,
        idempotency_key: &str,
        collection: &str,
        id: &str,
        expected_revision: u64,
    ) -> Result<GameDataWriteReceipt, SdkError> {
        self.mutate(&GameDataWriteRequest {
            idempotency_key: idempotency_key.into(),
            mutations: vec![GameDataMutation::Delete {
                collection: collection.into(),
                id: id.into(),
                expected_revision,
            }],
        })
        .await
    }
    /// Apply a bounded atomic multi-record compare-and-swap request.
    pub async fn mutate(
        &self,
        request: &GameDataWriteRequest,
    ) -> Result<GameDataWriteReceipt, SdkError> {
        if request.idempotency_key.is_empty()
            || request.idempotency_key.len() > 128
            || !request
                .idempotency_key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
            || request.mutations.is_empty()
            || request.mutations.len() > 25
        {
            return Err(SdkError::Validation(
                "A valid idempotency key and 1 to 25 mutations are required.".into(),
            ));
        }
        self.call("mutate", serde_json::to_value(request)?)
            .await?
            .ok_or_else(|| SdkError::PlatformData {
                status: 200,
                code: "GAME_DATA_INVALID_RESPONSE".into(),
            })
    }
    /// Query one declared index using bounded filters and stable keyset pagination.
    pub async fn query(&self, request: &GameDataQuery) -> Result<GameDataQueryResult, SdkError> {
        if !(1..=100).contains(&request.limit) {
            return Err(SdkError::Validation(
                "limit must be between 1 and 100.".into(),
            ));
        }
        self.call("query", serde_json::to_value(request)?)
            .await?
            .ok_or_else(|| SdkError::PlatformData {
                status: 200,
                code: "GAME_DATA_INVALID_RESPONSE".into(),
            })
    }
    /// Compute one supported aggregate with an explicit bounded scan limit.
    pub async fn aggregate(
        &self,
        request: &GameDataAggregate,
    ) -> Result<GameDataAggregateResult, SdkError> {
        if !(1..=1000).contains(&request.scan_limit) {
            return Err(SdkError::Validation(
                "scanLimit must be between 1 and 1000.".into(),
            ));
        }
        self.call("aggregate", serde_json::to_value(request)?)
            .await?
            .ok_or_else(|| SdkError::PlatformData {
                status: 200,
                code: "GAME_DATA_INVALID_RESPONSE".into(),
            })
    }
}
