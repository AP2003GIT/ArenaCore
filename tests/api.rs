use std::{collections::HashSet, sync::Arc};

use arena_core::{
    app::build_app, service::player_service::PlayerService,
    storage::memory::InMemoryPlayerRepository,
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tokio::task::JoinSet;
use tower::ServiceExt;
use uuid::Uuid;

fn test_app() -> Router {
    let repository = Arc::new(InMemoryPlayerRepository::new());
    let player_service = PlayerService::new(repository);
    build_app(player_service)
}

fn json_request(method: &str, uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

async fn response_json(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn health_returns_ok() {
    let response = test_app()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_json(response).await, json!({ "status": "ok" }));
}

#[tokio::test]
async fn creates_and_reads_a_player_with_default_rating() {
    let app = test_app();
    let response = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/v1/players",
            json!({ "display_name": "  Ada  " }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let created = response_json(response).await;
    let player_id = created["id"].as_str().unwrap();
    assert_eq!(created["display_name"], "Ada");
    assert_eq!(created["rating"], 1200);

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/players/{player_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_json(response).await, created);
}

#[tokio::test]
async fn missing_player_returns_machine_readable_404() {
    let response = test_app()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/players/{}", Uuid::new_v4()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response_json(response).await,
        json!({
            "error": {
                "code": "player_not_found",
                "message": "player was not found"
            }
        })
    );
}

#[tokio::test]
async fn invalid_fields_return_consistent_validation_errors() {
    let app = test_app();
    let cases = [
        (
            json!({ "display_name": "  ", "rating": 1200 }),
            "invalid_display_name",
        ),
        (
            json!({ "display_name": "Ada", "rating": 3001 }),
            "invalid_rating",
        ),
    ];

    for (body, expected_code) in cases {
        let response = app
            .clone()
            .oneshot(json_request("POST", "/api/v1/players", body))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            response_json(response).await["error"]["code"],
            expected_code
        );
    }
}

#[tokio::test]
async fn malformed_json_returns_error_envelope() {
    let response = test_app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/players")
                .header("content-type", "application/json")
                .body(Body::from("{"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response_json(response).await["error"]["code"],
        "invalid_json"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_create_requests_produce_unique_players() {
    let app = test_app();
    let mut tasks = JoinSet::new();

    for index in 0..32 {
        let app = app.clone();
        tasks.spawn(async move {
            app.oneshot(json_request(
                "POST",
                "/api/v1/players",
                json!({ "display_name": format!("Player {index:02}") }),
            ))
            .await
            .unwrap()
        });
    }

    let mut ids = HashSet::new();
    while let Some(response) = tasks.join_next().await {
        let response = response.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let player = response_json(response).await;
        ids.insert(player["id"].as_str().unwrap().to_owned());
    }

    assert_eq!(ids.len(), 32);
}
