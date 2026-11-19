use std::{collections::HashSet, sync::Arc};

use arena_core::{
    app::{build_app, AppState},
    config::WebSocketConfig,
    domain::matchmaking::MatchmakingConfig,
    service::{
        dev_session_service::DevSessionService,
        lobby_service::LobbyService,
        matchmaking_service::{MatchmakingService, SystemClock},
        notification_service::NotificationHub,
        player_service::PlayerService,
    },
    storage::{
        activity::PlayerActivityRegistry, lobby_memory::InMemoryLobbyRepository,
        matchmaking_memory::InMemoryMatchmakingRepository, memory::InMemoryPlayerRepository,
        LobbyRepository, MatchmakingRepository, PlayerRepository,
    },
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
    let activity_registry = Arc::new(PlayerActivityRegistry::new());
    let player_repository: Arc<dyn PlayerRepository> = Arc::new(InMemoryPlayerRepository::new());
    let lobby_repository: Arc<dyn LobbyRepository> =
        Arc::new(InMemoryLobbyRepository::new(Arc::clone(&activity_registry)));
    let matchmaking_repository: Arc<dyn MatchmakingRepository> =
        Arc::new(InMemoryMatchmakingRepository::new(activity_registry));
    let player_service = PlayerService::new(Arc::clone(&player_repository));
    let notification_hub = Arc::new(NotificationHub::default());
    let lobby_service = LobbyService::new(
        Arc::clone(&player_repository),
        lobby_repository,
        Arc::clone(&notification_hub),
    );
    let matchmaking_service = MatchmakingService::new(
        Arc::clone(&player_repository),
        matchmaking_repository,
        Arc::new(SystemClock),
        MatchmakingConfig::default(),
        Arc::clone(&notification_hub),
    );
    let dev_session_service = DevSessionService::new(player_repository);
    build_app(AppState {
        player_service,
        lobby_service,
        matchmaking_service,
        dev_session_service,
        notification_hub,
        websocket_config: WebSocketConfig::default(),
    })
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

async fn create_test_player(app: &Router, display_name: &str) -> Uuid {
    create_test_player_with_rating(app, display_name, None).await
}

async fn create_test_player_with_rating(
    app: &Router,
    display_name: &str,
    rating: Option<i32>,
) -> Uuid {
    let response = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/v1/players",
            json!({ "display_name": display_name, "rating": rating }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let player = response_json(response).await;
    Uuid::parse_str(player["id"].as_str().unwrap()).unwrap()
}

async fn create_test_lobby(app: &Router, owner_player_id: Uuid, capacity: Option<i32>) -> Value {
    let response = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/v1/lobbies",
            json!({
                "owner_player_id": owner_player_id,
                "capacity": capacity
            }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    response_json(response).await
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

#[tokio::test]
async fn creates_and_reads_a_lobby_with_owner_membership() {
    let app = test_app();
    let owner_id = create_test_player(&app, "Lobby Owner").await;
    let created = create_test_lobby(&app, owner_id, None).await;

    assert_eq!(created["owner_player_id"], owner_id.to_string());
    assert_eq!(created["capacity"], 4);
    assert_eq!(created["status"], "open");
    assert_eq!(created["members"].as_array().unwrap().len(), 1);
    assert_eq!(created["members"][0]["player_id"], owner_id.to_string());
    assert_eq!(created["members"][0]["ready"], false);

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/lobbies/{}",
                    created["id"].as_str().unwrap()
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_json(response).await, created);
}

#[tokio::test]
async fn validates_lobby_capacity_and_player_existence() {
    let app = test_app();
    let owner_id = create_test_player(&app, "Capacity Owner").await;
    let invalid_capacity = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/v1/lobbies",
            json!({ "owner_player_id": owner_id, "capacity": 1 }),
        ))
        .await
        .unwrap();

    assert_eq!(invalid_capacity.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        response_json(invalid_capacity).await["error"]["code"],
        "invalid_lobby_capacity"
    );

    let oversized_capacity = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/v1/lobbies",
            json!({ "owner_player_id": owner_id, "capacity": 300 }),
        ))
        .await
        .unwrap();
    assert_eq!(
        oversized_capacity.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        response_json(oversized_capacity).await["error"]["code"],
        "invalid_lobby_capacity"
    );

    let missing_player = app
        .oneshot(json_request(
            "POST",
            "/api/v1/lobbies",
            json!({ "owner_player_id": Uuid::new_v4() }),
        ))
        .await
        .unwrap();

    assert_eq!(missing_player.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response_json(missing_player).await["error"]["code"],
        "player_not_found"
    );
}

#[tokio::test]
async fn join_is_idempotent_and_ready_state_tracks_all_members() {
    let app = test_app();
    let owner_id = create_test_player(&app, "Ready Owner").await;
    let member_id = create_test_player(&app, "Ready Member").await;
    let lobby = create_test_lobby(&app, owner_id, Some(2)).await;
    let lobby_id = lobby["id"].as_str().unwrap();

    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(json_request(
                "POST",
                &format!("/api/v1/lobbies/{lobby_id}/join"),
                json!({ "player_id": member_id }),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response_json(response).await["members"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    let owner_ready = app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/lobbies/{lobby_id}/ready"),
            json!({ "player_id": owner_id, "ready": true }),
        ))
        .await
        .unwrap();
    assert_eq!(response_json(owner_ready).await["status"], "open");

    let all_ready = app
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/lobbies/{lobby_id}/ready"),
            json!({ "player_id": member_id, "ready": true }),
        ))
        .await
        .unwrap();
    assert_eq!(response_json(all_ready).await["status"], "ready");
}

#[tokio::test]
async fn owner_leave_transfers_ownership_and_last_leave_closes_lobby() {
    let app = test_app();
    let owner_id = create_test_player(&app, "Transfer Owner").await;
    let member_id = create_test_player(&app, "Transfer Member").await;
    let lobby = create_test_lobby(&app, owner_id, Some(4)).await;
    let lobby_id = lobby["id"].as_str().unwrap();

    app.clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/lobbies/{lobby_id}/join"),
            json!({ "player_id": member_id }),
        ))
        .await
        .unwrap();

    let owner_left = app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/lobbies/{lobby_id}/leave"),
            json!({ "player_id": owner_id }),
        ))
        .await
        .unwrap();
    let lobby_after_transfer = response_json(owner_left).await;
    assert_eq!(
        lobby_after_transfer["owner_player_id"],
        member_id.to_string()
    );

    let last_member_left = app
        .oneshot(json_request(
            "POST",
            &format!("/api/v1/lobbies/{lobby_id}/leave"),
            json!({ "player_id": member_id }),
        ))
        .await
        .unwrap();
    let closed_lobby = response_json(last_member_left).await;
    assert_eq!(closed_lobby["status"], "closed");
    assert_eq!(closed_lobby["owner_player_id"], Value::Null);
    assert!(closed_lobby["members"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn player_cannot_join_two_lobbies() {
    let app = test_app();
    let first_owner = create_test_player(&app, "First Owner").await;
    let second_owner = create_test_player(&app, "Second Owner").await;
    let member_id = create_test_player(&app, "Single Lobby Member").await;
    let first_lobby = create_test_lobby(&app, first_owner, None).await;
    let second_lobby = create_test_lobby(&app, second_owner, None).await;

    let first_join = app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!(
                "/api/v1/lobbies/{}/join",
                first_lobby["id"].as_str().unwrap()
            ),
            json!({ "player_id": member_id }),
        ))
        .await
        .unwrap();
    assert_eq!(first_join.status(), StatusCode::OK);

    let second_join = app
        .oneshot(json_request(
            "POST",
            &format!(
                "/api/v1/lobbies/{}/join",
                second_lobby["id"].as_str().unwrap()
            ),
            json!({ "player_id": member_id }),
        ))
        .await
        .unwrap();
    assert_eq!(second_join.status(), StatusCode::CONFLICT);
    assert_eq!(
        response_json(second_join).await["error"]["code"],
        "player_already_in_lobby"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_joins_never_exceed_lobby_capacity() {
    let app = test_app();
    let owner_id = create_test_player(&app, "Concurrent Owner").await;
    let lobby = create_test_lobby(&app, owner_id, Some(4)).await;
    let lobby_id = lobby["id"].as_str().unwrap().to_owned();
    let mut player_ids = Vec::new();

    for index in 0..12 {
        player_ids.push(create_test_player(&app, &format!("Joiner {index:02}")).await);
    }

    let mut tasks = JoinSet::new();
    for player_id in player_ids {
        let app = app.clone();
        let lobby_id = lobby_id.clone();
        tasks.spawn(async move {
            app.oneshot(json_request(
                "POST",
                &format!("/api/v1/lobbies/{lobby_id}/join"),
                json!({ "player_id": player_id }),
            ))
            .await
            .unwrap()
        });
    }

    let mut successful_joins = 0;
    let mut full_rejections = 0;
    while let Some(response) = tasks.join_next().await {
        let response = response.unwrap();
        match response.status() {
            StatusCode::OK => successful_joins += 1,
            StatusCode::CONFLICT => {
                assert_eq!(response_json(response).await["error"]["code"], "lobby_full");
                full_rejections += 1;
            }
            status => panic!("unexpected join status: {status}"),
        }
    }

    assert_eq!(successful_joins, 3);
    assert_eq!(full_rejections, 9);

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/lobbies/{lobby_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response_json(response).await["members"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
}

#[tokio::test]
async fn one_vs_one_queue_forms_a_readable_match() {
    let app = test_app();
    let first_player = create_test_player_with_rating(&app, "First Duelist", Some(1200)).await;
    let second_player = create_test_player_with_rating(&app, "Second Duelist", Some(1250)).await;

    let first_enqueue = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/v1/matchmaking/enqueue",
            json!({ "player_id": first_player, "mode": "1v1" }),
        ))
        .await
        .unwrap();
    assert_eq!(first_enqueue.status(), StatusCode::OK);
    assert_eq!(response_json(first_enqueue).await["status"], "queued");

    let second_enqueue = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/v1/matchmaking/enqueue",
            json!({ "player_id": second_player, "mode": "1v1" }),
        ))
        .await
        .unwrap();
    assert_eq!(second_enqueue.status(), StatusCode::OK);
    let matched = response_json(second_enqueue).await;
    assert_eq!(matched["status"], "matched");
    assert_eq!(matched["mode"], "1v1");
    let match_id = matched["match_id"].as_str().unwrap();

    let first_status = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/matchmaking/status/{first_player}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let first_status = response_json(first_status).await;
    assert_eq!(first_status["status"], "matched");
    assert_eq!(first_status["match_id"], match_id);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/matches/{match_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let game_match = response_json(response).await;
    assert_eq!(game_match["mode"], "1v1");
    assert_eq!(game_match["status"], "ready");
    assert_eq!(game_match["team_a"]["players"].as_array().unwrap().len(), 1);
    assert_eq!(game_match["team_b"]["players"].as_array().unwrap().len(), 1);

    let matched_player_lobby = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/v1/lobbies",
            json!({ "owner_player_id": first_player }),
        ))
        .await
        .unwrap();
    assert_eq!(matched_player_lobby.status(), StatusCode::CONFLICT);
    assert_eq!(
        response_json(matched_player_lobby).await["error"]["code"],
        "player_in_match"
    );

    let matched_player_requeue = app
        .oneshot(json_request(
            "POST",
            "/api/v1/matchmaking/enqueue",
            json!({ "player_id": first_player, "mode": "1v1" }),
        ))
        .await
        .unwrap();
    assert_eq!(matched_player_requeue.status(), StatusCode::CONFLICT);
    assert_eq!(
        response_json(matched_player_requeue).await["error"]["code"],
        "player_in_match"
    );
}

#[tokio::test]
async fn duplicate_queue_is_rejected_and_cancellation_is_idempotent() {
    let app = test_app();
    let player_id = create_test_player(&app, "Queue Canceller").await;

    let enqueue = || {
        json_request(
            "POST",
            "/api/v1/matchmaking/enqueue",
            json!({ "player_id": player_id, "mode": "1v1" }),
        )
    };

    let first = app.clone().oneshot(enqueue()).await.unwrap();
    assert_eq!(first.status(), StatusCode::OK);

    let duplicate = app.clone().oneshot(enqueue()).await.unwrap();
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);
    assert_eq!(
        response_json(duplicate).await["error"]["code"],
        "player_already_queued"
    );

    for _ in 0..2 {
        let cancelled = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/matchmaking/queue/{player_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(cancelled.status(), StatusCode::NO_CONTENT);
    }

    let status = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/matchmaking/status/{player_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response_json(status).await["status"], "idle");
}

#[tokio::test]
async fn lobby_and_queue_membership_are_mutually_exclusive() {
    let app = test_app();
    let queued_player = create_test_player(&app, "Queued Player").await;
    let lobby_player = create_test_player(&app, "Lobby Player").await;

    let queued = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/v1/matchmaking/enqueue",
            json!({ "player_id": queued_player, "mode": "1v1" }),
        ))
        .await
        .unwrap();
    assert_eq!(queued.status(), StatusCode::OK);

    let queued_player_lobby = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/v1/lobbies",
            json!({ "owner_player_id": queued_player }),
        ))
        .await
        .unwrap();
    assert_eq!(queued_player_lobby.status(), StatusCode::CONFLICT);
    assert_eq!(
        response_json(queued_player_lobby).await["error"]["code"],
        "player_already_queued"
    );

    create_test_lobby(&app, lobby_player, None).await;
    let lobby_player_queue = app
        .oneshot(json_request(
            "POST",
            "/api/v1/matchmaking/enqueue",
            json!({ "player_id": lobby_player, "mode": "1v1" }),
        ))
        .await
        .unwrap();
    assert_eq!(lobby_player_queue.status(), StatusCode::CONFLICT);
    assert_eq!(
        response_json(lobby_player_queue).await["error"]["code"],
        "player_in_lobby"
    );
}

#[tokio::test]
async fn two_vs_two_match_uses_deterministic_rating_balance() {
    let app = test_app();
    let ratings = [1000, 1040, 1060, 1100];
    let mut final_status = Value::Null;

    for (index, rating) in ratings.into_iter().enumerate() {
        let player_id =
            create_test_player_with_rating(&app, &format!("Team Player {index}"), Some(rating))
                .await;
        let response = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/v1/matchmaking/enqueue",
                json!({ "player_id": player_id, "mode": "2v2" }),
            ))
            .await
            .unwrap();
        final_status = response_json(response).await;
    }

    assert_eq!(final_status["status"], "matched");
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/matches/{}",
                    final_status["match_id"].as_str().unwrap()
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let game_match = response_json(response).await;

    assert_eq!(game_match["mode"], "2v2");
    assert_eq!(game_match["team_a"]["total_rating"], 2100);
    assert_eq!(game_match["team_b"]["total_rating"], 2100);
}

#[tokio::test]
async fn invalid_matchmaking_mode_is_a_validation_error() {
    let app = test_app();
    let player_id = create_test_player(&app, "Invalid Mode Player").await;
    let response = app
        .oneshot(json_request(
            "POST",
            "/api/v1/matchmaking/enqueue",
            json!({ "player_id": player_id, "mode": "3v3" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        response_json(response).await["error"]["code"],
        "invalid_match_mode"
    );
}
