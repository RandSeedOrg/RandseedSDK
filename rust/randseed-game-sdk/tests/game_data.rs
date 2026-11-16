use axum::{routing::post, Json, Router};
use randseed_game_sdk::{
    GameDataAggregate, GameDataAggregateMetric, GameDataAggregateOperator, GameDataFieldType,
    GameDataFilter, GameDataFilterOperator, GameDataQuery, GameDataSchema, GameDataSort,
    GameDataSortDirection, PlatformClient, SdkError,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};

#[tokio::test]
async fn game_data_v1_uses_versioned_shapes_and_sanitized_errors() {
    let router = Router::new()
        .route("/platform.data/schema", post(|Json(body): Json<Value>| async move {
            assert_eq!(body["protocolVersion"], 1);
            assert_eq!(body["request"]["schema"]["compoundIndexes"][0][0], "season");
            assert_eq!(body["request"]["schema"]["compoundIndexes"][0][1], "score");
            (axum::http::StatusCode::OK, Json(json!({"success": true, "data": {"registered": true}})))
        }))
        .route("/platform.data/get", post(|Json(body): Json<Value>| async move {
            assert_eq!(body["protocolVersion"], 1);
            (axum::http::StatusCode::OK, Json(json!({"success": true, "data": {"id": body["request"]["id"], "value": {"score": 7}, "revision": 3, "deleted": false}})))
        }))
        .route("/platform.data/mutate", post(|Json(body): Json<Value>| async move {
            assert_eq!(body["protocolVersion"], 1);
            let mutation = &body["request"]["mutations"][0];
            let (id, revision) = if mutation["id"] == "alice" {
                assert_eq!(mutation["expectedRevision"], 3);
                ("alice", 4)
            } else {
                assert_eq!(mutation["id"], "precision");
                assert_eq!(mutation["expectedRevision"], 0);
                assert_eq!(mutation["value"]["score"].as_f64(), Some(1_000_000_000_000_000.5));
                ("precision", 1)
            };
            (axum::http::StatusCode::OK, Json(json!({"success": true, "data": {"idempotencyKey": body["request"]["idempotencyKey"], "items": [{"collection": "scores", "id": id, "revision": revision, "deleted": false}]}})))
        }))
        .route("/platform.data/query", post(|Json(body): Json<Value>| async move {
            assert_eq!(body["protocolVersion"], 1);
            if body["request"]["collection"] == "error" {
                return (axum::http::StatusCode::CONFLICT, Json(json!({"success": false, "code": "DATA_REVISION_CONFLICT", "message": "private backend details"})));
            }
            assert_eq!(body["request"]["where"][0]["field"], "season");
            assert_eq!(body["request"]["where"][0]["op"], "eq");
            assert_eq!(body["request"]["orderBy"]["field"], "score");
            (axum::http::StatusCode::OK, Json(json!({"success": true, "data": {"items": [], "nextCursor": null}})))
        }))
        .route("/platform.data/aggregate", post(|Json(body): Json<Value>| async move {
            assert_eq!(body["protocolVersion"], 1);
            assert_eq!(body["request"]["scanLimit"], 100);
            (axum::http::StatusCode::OK, Json(json!({"success": true, "data": {"value": 7.0, "scanned": 1}})))
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let mut map = HashMap::new();
    map.insert(
        "platform.data".to_owned(),
        format!("{}/platform.data", base),
    );
    let data = PlatformClient::with_vhost_map(map).data();
    let schema = GameDataSchema {
        version: 1,
        fields: BTreeMap::from([
            ("score".to_owned(), GameDataFieldType::Number),
            ("season".to_owned(), GameDataFieldType::Number),
        ]),
        indexes: vec!["score".to_owned()],
        compound_indexes: vec![["season".to_owned(), "score".to_owned()]],
    };
    data.register_schema("scores", &schema).await.unwrap();
    let record = data.get("scores", "alice").await.unwrap().unwrap();
    assert_eq!(record.revision, 3);
    let receipt = data
        .put("write-1", "scores", "alice", 3, json!({"score": 8}))
        .await
        .unwrap();
    assert_eq!(receipt.items[0].revision, 4);
    let precision_receipt = data
        .put(
            "write-precision",
            "scores",
            "precision",
            0,
            json!({"score": 1_000_000_000_000_000.5}),
        )
        .await
        .unwrap();
    assert_eq!(precision_receipt.items[0].revision, 1);
    let query = GameDataQuery {
        collection: "scores".into(),
        filters: vec![GameDataFilter {
            field: "season".into(),
            op: GameDataFilterOperator::Eq,
            value: json!(4),
        }],
        order_by: Some(GameDataSort {
            field: "score".into(),
            direction: GameDataSortDirection::Desc,
        }),
        limit: 10,
        cursor: None,
    };
    assert!(data.query(&query).await.unwrap().items.is_empty());
    let aggregate = GameDataAggregate {
        collection: "scores".into(),
        filters: vec![],
        metric: GameDataAggregateMetric {
            op: GameDataAggregateOperator::Sum,
            field: Some("score".into()),
        },
        scan_limit: 100,
    };
    assert_eq!(data.aggregate(&aggregate).await.unwrap().value, Some(7.0));
    let invalid = GameDataQuery {
        collection: "error".into(),
        filters: vec![],
        order_by: None,
        limit: 1,
        cursor: None,
    };
    let error = data.query(&invalid).await.unwrap_err();
    assert!(
        matches!(error, SdkError::PlatformData { status: 409, ref code } if code == "DATA_REVISION_CONFLICT")
    );
    assert!(!error.to_string().contains("private backend details"));
    assert!(!error.retryable());
    assert_eq!(error.platform_data_code(), Some("DATA_REVISION_CONFLICT"));
    assert!(!SdkError::PlatformData {
        status: 503,
        code: "GAME_DATA_DISABLED".into()
    }
    .retryable());
    assert!(!SdkError::PlatformData {
        status: 507,
        code: "DATA_CAPACITY_REACHED".into()
    }
    .retryable());
    assert!(!error.creator_message().contains("private backend details"));
    assert!(matches!(
        data.query(&GameDataQuery {
            limit: 101,
            ..invalid
        })
        .await,
        Err(SdkError::Validation(_))
    ));
}
