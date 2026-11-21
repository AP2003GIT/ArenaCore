use std::{sync::Arc, time::Duration};

use arena_core::{
    app::{build_app, AppState},
    config::{RateLimitConfig, WebSocketConfig},
    domain::matchmaking::MatchmakingConfig,
    operations::{OperationalMetrics, RateLimiter},
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
        readiness::InMemoryReadiness, LobbyRepository, MatchmakingRepository, PlayerRepository,
    },
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use futures_util::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tokio::{
    net::{TcpListener, TcpStream},
    task::JoinHandle,
    time,
};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, http::HeaderValue, protocol::Message, Error},
    MaybeTlsStream, WebSocketStream,
};
use tower::ServiceExt;
use uuid::Uuid;

type TestSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct TestApp {
    router: Router,
    notifications: Arc<NotificationHub>,
}

fn test_app() -> TestApp {
    let activity_registry = Arc::new(PlayerActivityRegistry::new());
    let player_repository: Arc<dyn PlayerRepository> = Arc::new(InMemoryPlayerRepository::new());
    let lobby_repository: Arc<dyn LobbyRepository> =
        Arc::new(InMemoryLobbyRepository::new(Arc::clone(&activity_registry)));
    let matchmaking_repository: Arc<dyn MatchmakingRepository> =
        Arc::new(InMemoryMatchmakingRepository::new(activity_registry));
    let notifications = Arc::new(NotificationHub::default());
    let player_service = PlayerService::new(Arc::clone(&player_repository));
    let lobby_service = LobbyService::new(
        Arc::clone(&player_repository),
        lobby_repository,
        Arc::clone(&notifications),
    );
    let matchmaking_service = MatchmakingService::new(
        Arc::clone(&player_repository),
        matchmaking_repository,
        Arc::new(SystemClock),
        MatchmakingConfig::default(),
        Arc::clone(&notifications),
    );
    let dev_session_service = DevSessionService::new(player_repository);
    let websocket_config =
        WebSocketConfig::new(8, Duration::from_millis(50), Duration::from_millis(250)).unwrap();
    let metrics = Arc::new(OperationalMetrics::default());
    let rate_limiter = Arc::new(RateLimiter::new(
        RateLimitConfig::default(),
        Arc::clone(&metrics),
    ));
    let router = build_app(AppState {
        player_service,
        lobby_service,
        matchmaking_service,
        dev_session_service,
        notification_hub: Arc::clone(&notifications),
        websocket_config,
        readiness_probe: Arc::new(InMemoryReadiness),
        metrics,
        rate_limiter,
    });

    TestApp {
        router,
        notifications,
    }
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

async fn create_player(app: &Router, name: &str) -> Uuid {
    let response = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/v1/players",
            json!({ "display_name": name }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    Uuid::parse_str(response_json(response).await["id"].as_str().unwrap()).unwrap()
}

async fn issue_token(app: &Router, player_id: Uuid) -> Uuid {
    let response = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/v1/dev/sessions",
            json!({ "player_id": player_id }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    Uuid::parse_str(
        response_json(response).await["access_token"]
            .as_str()
            .unwrap(),
    )
    .unwrap()
}

async fn start_server(app: Router) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("ws://{address}"), task)
}

async fn connect(url: &str, player_id: Uuid, token: Uuid) -> TestSocket {
    let mut request = format!("{url}/api/v1/ws?player_id={player_id}")
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
    );
    connect_async(request).await.unwrap().0
}

async fn next_event(socket: &mut TestSocket) -> Value {
    time::timeout(Duration::from_secs(2), async {
        loop {
            match socket.next().await.unwrap().unwrap() {
                Message::Text(text) => return serde_json::from_str(&text).unwrap(),
                Message::Ping(payload) => socket.send(Message::Pong(payload)).await.unwrap(),
                Message::Close(frame) => panic!("socket closed before event: {frame:?}"),
                _ => {}
            }
        }
    })
    .await
    .expect("timed out waiting for WebSocket event")
}

#[tokio::test]
async fn websocket_rejects_missing_or_wrong_player_token() {
    let test_app = test_app();
    let first_player = create_player(&test_app.router, "First Socket Player").await;
    let second_player = create_player(&test_app.router, "Second Socket Player").await;
    let token = issue_token(&test_app.router, first_player).await;
    let (url, server) = start_server(test_app.router).await;

    let missing_token = connect_async(format!("{url}/api/v1/ws?player_id={first_player}"))
        .await
        .unwrap_err();
    assert!(matches!(missing_token, Error::Http(response) if response.status().as_u16() == 401));

    let mut wrong_player_request = format!("{url}/api/v1/ws?player_id={second_player}")
        .into_client_request()
        .unwrap();
    wrong_player_request.headers_mut().insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
    );
    let wrong_player = connect_async(wrong_player_request).await.unwrap_err();
    assert!(matches!(wrong_player, Error::Http(response) if response.status().as_u16() == 401));

    server.abort();
}

#[tokio::test]
async fn websocket_delivers_matchmaking_events_and_cleans_up_disconnect() {
    let test_app = test_app();
    let first_player = create_player(&test_app.router, "Connected Duelist").await;
    let second_player = create_player(&test_app.router, "Matching Duelist").await;
    let token = issue_token(&test_app.router, first_player).await;
    let (url, server) = start_server(test_app.router.clone()).await;
    let mut socket = connect(&url, first_player, token).await;

    time::timeout(Duration::from_secs(1), async {
        while test_app.notifications.connection_count(first_player) != 1 {
            time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("WebSocket subscription was not registered");

    let queued = test_app
        .router
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/v1/matchmaking/enqueue",
            json!({ "player_id": first_player, "mode": "1v1" }),
        ))
        .await
        .unwrap();
    assert_eq!(queued.status(), StatusCode::OK);
    let queued_event = next_event(&mut socket).await;
    assert_eq!(queued_event["type"], "queue_updated");
    assert_eq!(queued_event["payload"]["status"], "queued");

    let matched = test_app
        .router
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/v1/matchmaking/enqueue",
            json!({ "player_id": second_player, "mode": "1v1" }),
        ))
        .await
        .unwrap();
    assert_eq!(matched.status(), StatusCode::OK);

    let match_status_event = next_event(&mut socket).await;
    assert_eq!(match_status_event["type"], "queue_updated");
    assert_eq!(match_status_event["payload"]["status"], "matched");
    let match_event = next_event(&mut socket).await;
    assert_eq!(match_event["type"], "match_found");
    assert_eq!(
        match_event["payload"]["match_id"],
        match_status_event["payload"]["match_id"]
    );

    socket.close(None).await.unwrap();
    time::timeout(Duration::from_secs(1), async {
        while test_app.notifications.connection_count(first_player) != 0 {
            time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("WebSocket subscription was not removed after disconnect");

    server.abort();
}

#[tokio::test]
async fn websocket_heartbeat_accepts_pong() {
    let test_app = test_app();
    let player_id = create_player(&test_app.router, "Heartbeat Player").await;
    let token = issue_token(&test_app.router, player_id).await;
    let (url, server) = start_server(test_app.router).await;
    let mut socket = connect(&url, player_id, token).await;

    let message = time::timeout(Duration::from_secs(1), socket.next())
        .await
        .expect("server did not send a heartbeat")
        .unwrap()
        .unwrap();
    let Message::Ping(payload) = message else {
        panic!("expected heartbeat ping, got {message:?}");
    };
    socket.send(Message::Pong(payload)).await.unwrap();
    time::sleep(Duration::from_millis(100)).await;
    assert_eq!(test_app.notifications.connection_count(player_id), 1);

    socket.close(None).await.unwrap();
    server.abort();
}
