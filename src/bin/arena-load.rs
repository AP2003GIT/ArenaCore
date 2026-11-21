use std::{
    collections::{BTreeMap, HashSet},
    env,
    error::Error,
    fs, io,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use futures_util::{stream, SinkExt, StreamExt, TryStreamExt};
use reqwest::{Client, RequestBuilder};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::{net::TcpStream, time as tokio_time};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        client::IntoClientRequest,
        http::{header::AUTHORIZATION, HeaderValue},
        Message,
    },
    MaybeTlsStream, WebSocketStream,
};
use uuid::Uuid;

type HarnessError = Box<dyn Error + Send + Sync>;
type HarnessResult<T> = Result<T, HarnessError>;
type ClientSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[tokio::main]
async fn main() -> HarnessResult<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let arguments = Arguments::parse(env::args().skip(1))?;
    if arguments.help {
        print_help();
        return Ok(());
    }

    let scenario_path = arguments
        .scenario
        .as_deref()
        .ok_or_else(|| harness_error("--scenario <path> is required"))?;
    let source = fs::read_to_string(scenario_path).map_err(|error| {
        harness_error(format!(
            "failed to read scenario {}: {error}",
            scenario_path.display()
        ))
    })?;
    let scenario: Scenario = serde_json::from_str(&source).map_err(|error| {
        harness_error(format!(
            "failed to parse scenario {}: {error}",
            scenario_path.display()
        ))
    })?;
    scenario.validate()?;

    let report = run_scenario(&scenario).await?;
    let rendered = serde_json::to_string_pretty(&report)?;
    println!("{rendered}");

    if let Some(output) = arguments.output {
        if let Some(parent) = output
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        fs::write(&output, format!("{rendered}\n"))?;
        eprintln!("report written to {}", output.display());
    }

    if report.failures > 0 {
        return Err(harness_error(format!(
            "scenario completed with {} failed samples",
            report.failures
        )));
    }

    Ok(())
}

fn print_help() {
    println!(
        "ArenaCore load and integration simulator\n\n\
         Usage:\n  cargo run --release --bin arena-load -- \\\n+           --scenario load/scenarios/smoke.json \\\n+           [--output target/load-reports/smoke.json]\n\n\
         The scenario file controls the target URL, concurrency, timeout, and workload."
    );
}

#[derive(Debug, Default)]
struct Arguments {
    scenario: Option<PathBuf>,
    output: Option<PathBuf>,
    help: bool,
}

impl Arguments {
    fn parse(arguments: impl IntoIterator<Item = String>) -> HarnessResult<Self> {
        let mut parsed = Self::default();
        let mut arguments = arguments.into_iter();

        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--scenario" => {
                    parsed.scenario =
                        Some(PathBuf::from(arguments.next().ok_or_else(|| {
                            harness_error("--scenario requires a path")
                        })?));
                }
                "--output" => {
                    parsed.output = Some(PathBuf::from(
                        arguments
                            .next()
                            .ok_or_else(|| harness_error("--output requires a path"))?,
                    ));
                }
                "-h" | "--help" => parsed.help = true,
                _ => return Err(harness_error(format!("unknown argument: {argument}"))),
            }
        }

        Ok(parsed)
    }
}

#[derive(Clone, Debug, Deserialize)]
struct Scenario {
    name: String,
    base_url: String,
    concurrency: usize,
    timeout_seconds: u64,
    workload: Workload,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Workload {
    Health {
        requests: usize,
    },
    Matchmaking {
        players: usize,
        mode: MatchMode,
        connect_websockets: bool,
        rating_seed: u64,
        rating_center: u16,
        rating_spread: u16,
        poll_interval_ms: u64,
    },
}

#[derive(Clone, Copy, Debug, Deserialize)]
enum MatchMode {
    #[serde(rename = "1v1")]
    OneVsOne,
    #[serde(rename = "2v2")]
    TwoVsTwo,
}

impl MatchMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::OneVsOne => "1v1",
            Self::TwoVsTwo => "2v2",
        }
    }

    fn player_count(self) -> usize {
        match self {
            Self::OneVsOne => 2,
            Self::TwoVsTwo => 4,
        }
    }
}

impl Scenario {
    fn validate(&self) -> HarnessResult<()> {
        if self.name.trim().is_empty() {
            return Err(harness_error("scenario name cannot be empty"));
        }
        if !(self.base_url.starts_with("http://") || self.base_url.starts_with("https://")) {
            return Err(harness_error(
                "base_url must start with http:// or https://",
            ));
        }
        if self.concurrency == 0 {
            return Err(harness_error("concurrency must be greater than zero"));
        }
        if self.timeout_seconds == 0 {
            return Err(harness_error("timeout_seconds must be greater than zero"));
        }

        match self.workload {
            Workload::Health { requests: 0 } => {
                Err(harness_error("health requests must be greater than zero"))
            }
            Workload::Matchmaking {
                players,
                mode,
                rating_center,
                rating_spread,
                poll_interval_ms,
                ..
            } => {
                if players == 0 || !players.is_multiple_of(mode.player_count()) {
                    return Err(harness_error(format!(
                        "players must be a positive multiple of {} for {}",
                        mode.player_count(),
                        mode.as_str()
                    )));
                }
                if poll_interval_ms == 0 {
                    return Err(harness_error("poll_interval_ms must be greater than zero"));
                }
                if rating_spread > rating_center
                    || u32::from(rating_center) + u32::from(rating_spread) > 3000
                {
                    return Err(harness_error("rating range must stay between 0 and 3000"));
                }
                Ok(())
            }
            Workload::Health { .. } => Ok(()),
        }
    }

    fn workload_name(&self) -> &'static str {
        match self.workload {
            Workload::Health { .. } => "health",
            Workload::Matchmaking { .. } => "matchmaking",
        }
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_seconds)
    }
}

#[derive(Clone, Copy)]
enum SampleKind {
    Http,
    WebSocket,
    Outcome,
}

#[derive(Clone)]
struct Recorder {
    samples: Arc<Mutex<Vec<Sample>>>,
}

struct Sample {
    kind: SampleKind,
    operation: &'static str,
    elapsed: Duration,
    success: bool,
}

impl Recorder {
    fn new() -> Self {
        Self {
            samples: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn record(&self, kind: SampleKind, operation: &'static str, elapsed: Duration, success: bool) {
        self.samples
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(Sample {
                kind,
                operation,
                elapsed,
                success,
            });
    }

    fn snapshot(&self) -> Vec<SampleSnapshot> {
        self.samples
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .map(SampleSnapshot::from)
            .collect()
    }
}

struct SampleSnapshot {
    kind: SampleKind,
    operation: &'static str,
    elapsed_micros: u64,
    success: bool,
}

impl From<&Sample> for SampleSnapshot {
    fn from(sample: &Sample) -> Self {
        Self {
            kind: sample.kind,
            operation: sample.operation,
            elapsed_micros: duration_micros(sample.elapsed),
            success: sample.success,
        }
    }
}

#[derive(Serialize)]
struct Report {
    scenario: String,
    workload: &'static str,
    base_url: String,
    started_at: String,
    elapsed_ms: u64,
    throughput_per_second: f64,
    http_requests: usize,
    websocket_handshakes: usize,
    failures: usize,
    matches_formed: usize,
    match_found_events: usize,
    operations: BTreeMap<&'static str, OperationSummary>,
}

#[derive(Serialize)]
struct OperationSummary {
    samples: usize,
    succeeded: usize,
    failed: usize,
    min_ms: f64,
    mean_ms: f64,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    max_ms: f64,
}

impl Report {
    fn build(
        scenario: &Scenario,
        recorder: &Recorder,
        started_at: String,
        elapsed: Duration,
        matches_formed: usize,
        match_found_events: usize,
    ) -> Self {
        let samples = recorder.snapshot();
        let http_requests = samples
            .iter()
            .filter(|sample| matches!(sample.kind, SampleKind::Http))
            .count();
        let websocket_handshakes = samples
            .iter()
            .filter(|sample| matches!(sample.kind, SampleKind::WebSocket))
            .count();
        let failures = samples.iter().filter(|sample| !sample.success).count();
        let mut grouped: BTreeMap<&'static str, Vec<&SampleSnapshot>> = BTreeMap::new();
        for sample in &samples {
            grouped.entry(sample.operation).or_default().push(sample);
        }
        let operations = grouped
            .into_iter()
            .map(|(name, samples)| (name, OperationSummary::from_samples(&samples)))
            .collect();
        let seconds = elapsed.as_secs_f64();

        Self {
            scenario: scenario.name.clone(),
            workload: scenario.workload_name(),
            base_url: scenario.base_url.clone(),
            started_at,
            elapsed_ms: duration_millis(elapsed),
            throughput_per_second: if seconds > 0.0 {
                http_requests as f64 / seconds
            } else {
                0.0
            },
            http_requests,
            websocket_handshakes,
            failures,
            matches_formed,
            match_found_events,
            operations,
        }
    }
}

impl OperationSummary {
    fn from_samples(samples: &[&SampleSnapshot]) -> Self {
        let mut durations: Vec<u64> = samples.iter().map(|sample| sample.elapsed_micros).collect();
        durations.sort_unstable();
        let total: u128 = durations.iter().map(|value| u128::from(*value)).sum();
        let succeeded = samples.iter().filter(|sample| sample.success).count();
        let mean = if durations.is_empty() {
            0.0
        } else {
            total as f64 / durations.len() as f64
        };

        Self {
            samples: samples.len(),
            succeeded,
            failed: samples.len() - succeeded,
            min_ms: micros_to_ms(durations.first().copied().unwrap_or(0)),
            mean_ms: mean / 1_000.0,
            p50_ms: micros_to_ms(percentile(&durations, 50)),
            p95_ms: micros_to_ms(percentile(&durations, 95)),
            p99_ms: micros_to_ms(percentile(&durations, 99)),
            max_ms: micros_to_ms(durations.last().copied().unwrap_or(0)),
        }
    }
}

async fn run_scenario(scenario: &Scenario) -> HarnessResult<Report> {
    let client = Client::builder().timeout(scenario.timeout()).build()?;
    let recorder = Recorder::new();
    let started_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned());

    request_text(
        &recorder,
        "readiness",
        client.get(endpoint(&scenario.base_url, "/ready")),
    )
    .await?;

    let started = Instant::now();
    let outcome = match scenario.workload {
        Workload::Health { requests } => {
            run_health_workload(scenario, &client, &recorder, requests).await;
            WorkloadOutcome::default()
        }
        Workload::Matchmaking {
            players,
            mode,
            connect_websockets,
            rating_seed,
            rating_center,
            rating_spread,
            poll_interval_ms,
        } => {
            run_matchmaking_workload(
                scenario,
                &client,
                &recorder,
                MatchmakingOptions {
                    players,
                    mode,
                    connect_websockets,
                    rating_seed,
                    rating_center,
                    rating_spread,
                    poll_interval: Duration::from_millis(poll_interval_ms),
                },
            )
            .await?
        }
    };
    let elapsed = started.elapsed();

    Ok(Report::build(
        scenario,
        &recorder,
        started_at,
        elapsed,
        outcome.matches_formed,
        outcome.match_found_events,
    ))
}

#[derive(Default)]
struct WorkloadOutcome {
    matches_formed: usize,
    match_found_events: usize,
}

async fn run_health_workload(
    scenario: &Scenario,
    client: &Client,
    recorder: &Recorder,
    requests: usize,
) {
    stream::iter(0..requests)
        .for_each_concurrent(scenario.concurrency, |_| {
            let client = client.clone();
            let recorder = recorder.clone();
            let url = endpoint(&scenario.base_url, "/health");
            async move {
                let _ = request_text(&recorder, "health", client.get(url)).await;
            }
        })
        .await;
}

#[derive(Clone, Copy)]
struct MatchmakingOptions {
    players: usize,
    mode: MatchMode,
    connect_websockets: bool,
    rating_seed: u64,
    rating_center: u16,
    rating_spread: u16,
    poll_interval: Duration,
}

struct SimulatedPlayer {
    id: Uuid,
    socket: Option<ClientSocket>,
}

#[derive(Deserialize)]
struct PlayerResponse {
    id: Uuid,
}

#[derive(Deserialize)]
struct SessionResponse {
    access_token: Uuid,
}

#[derive(Deserialize)]
struct MatchmakingStatusResponse {
    status: String,
    match_id: Option<Uuid>,
}

async fn run_matchmaking_workload(
    scenario: &Scenario,
    client: &Client,
    recorder: &Recorder,
    options: MatchmakingOptions,
) -> HarnessResult<WorkloadOutcome> {
    let mut players: Vec<SimulatedPlayer> = stream::iter(0..options.players)
        .map(|index| {
            let client = client.clone();
            let recorder = recorder.clone();
            let base_url = scenario.base_url.clone();
            async move {
                let rating = deterministic_rating(
                    options.rating_seed,
                    index,
                    options.rating_center,
                    options.rating_spread,
                );
                let player: PlayerResponse = request_json(
                    &recorder,
                    "create_player",
                    client
                        .post(endpoint(&base_url, "/api/v1/players"))
                        .json(&json!({
                            "display_name": format!("load-{:04x}-{index:06}", options.rating_seed & 0xffff),
                            "rating": rating,
                        })),
                )
                .await?;
                Ok::<_, HarnessError>(SimulatedPlayer {
                    id: player.id,
                    socket: None,
                })
            }
        })
        .buffer_unordered(scenario.concurrency)
        .try_collect()
        .await?;

    if options.connect_websockets {
        players = stream::iter(players)
            .map(|mut player| {
                let client = client.clone();
                let recorder = recorder.clone();
                let base_url = scenario.base_url.clone();
                async move {
                    let session: SessionResponse = request_json(
                        &recorder,
                        "create_session",
                        client
                            .post(endpoint(&base_url, "/api/v1/dev/sessions"))
                            .json(&json!({ "player_id": player.id })),
                    )
                    .await?;
                    player.socket = Some(
                        connect_player_socket(
                            &recorder,
                            &base_url,
                            player.id,
                            session.access_token,
                        )
                        .await?,
                    );
                    Ok::<_, HarnessError>(player)
                }
            })
            .buffer_unordered(scenario.concurrency)
            .try_collect()
            .await?;
    }

    stream::iter(players.iter())
        .map(|player| {
            let client = client.clone();
            let recorder = recorder.clone();
            let base_url = scenario.base_url.clone();
            async move {
                let _: MatchmakingStatusResponse = request_json(
                    &recorder,
                    "enqueue",
                    client
                        .post(endpoint(&base_url, "/api/v1/matchmaking/enqueue"))
                        .json(&json!({
                            "player_id": player.id,
                            "mode": options.mode.as_str(),
                        })),
                )
                .await?;
                Ok::<_, HarnessError>(())
            }
        })
        .buffer_unordered(scenario.concurrency)
        .try_collect::<Vec<_>>()
        .await?;

    let assignment_started = Instant::now();
    let player_ids: Vec<Uuid> = players.iter().map(|player| player.id).collect();
    let sockets: Vec<ClientSocket> = players
        .iter_mut()
        .filter_map(|player| player.socket.take())
        .collect();
    let assignments = wait_for_assignments(
        scenario,
        client,
        recorder,
        &player_ids,
        options.poll_interval,
        assignment_started,
    );
    let events = wait_for_match_events(recorder, sockets, scenario.timeout(), assignment_started);
    let (match_ids, match_found_events) = tokio::try_join!(assignments, events)?;
    let matches_formed = match_ids.into_iter().collect::<HashSet<_>>().len();
    let expected_matches = options.players / options.mode.player_count();

    if matches_formed != expected_matches {
        return Err(harness_error(format!(
            "expected {expected_matches} matches but observed {matches_formed}"
        )));
    }
    if options.connect_websockets && match_found_events != options.players {
        return Err(harness_error(format!(
            "expected {} match_found events but observed {match_found_events}",
            options.players
        )));
    }

    Ok(WorkloadOutcome {
        matches_formed,
        match_found_events,
    })
}

async fn connect_player_socket(
    recorder: &Recorder,
    base_url: &str,
    player_id: Uuid,
    access_token: Uuid,
) -> HarnessResult<ClientSocket> {
    let websocket_url = websocket_endpoint(base_url, player_id)?;
    let mut request = websocket_url.into_client_request()?;
    request.headers_mut().insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {access_token}"))?,
    );
    let started = Instant::now();

    match connect_async(request).await {
        Ok((socket, _)) => {
            recorder.record(
                SampleKind::WebSocket,
                "websocket_connect",
                started.elapsed(),
                true,
            );
            Ok(socket)
        }
        Err(error) => {
            recorder.record(
                SampleKind::WebSocket,
                "websocket_connect",
                started.elapsed(),
                false,
            );
            Err(Box::new(error))
        }
    }
}

async fn wait_for_assignments(
    scenario: &Scenario,
    client: &Client,
    recorder: &Recorder,
    player_ids: &[Uuid],
    poll_interval: Duration,
    assignment_started: Instant,
) -> HarnessResult<Vec<Uuid>> {
    stream::iter(player_ids.iter().copied())
        .map(|player_id| {
            let client = client.clone();
            let recorder = recorder.clone();
            let base_url = scenario.base_url.clone();
            async move {
                let deadline = Instant::now() + scenario.timeout();
                loop {
                    let status: MatchmakingStatusResponse = request_json(
                        &recorder,
                        "matchmaking_status",
                        client.get(endpoint(
                            &base_url,
                            &format!("/api/v1/matchmaking/status/{player_id}"),
                        )),
                    )
                    .await?;

                    if status.status == "matched" {
                        let match_id = status
                            .match_id
                            .ok_or_else(|| harness_error("matched status omitted match_id"))?;
                        recorder.record(
                            SampleKind::Outcome,
                            "time_to_match",
                            assignment_started.elapsed(),
                            true,
                        );
                        return Ok::<_, HarnessError>(match_id);
                    }
                    if Instant::now() >= deadline {
                        recorder.record(
                            SampleKind::Outcome,
                            "time_to_match",
                            assignment_started.elapsed(),
                            false,
                        );
                        return Err(harness_error(format!(
                            "player {player_id} was not matched before the timeout"
                        )));
                    }
                    tokio_time::sleep(poll_interval).await;
                }
            }
        })
        .buffer_unordered(scenario.concurrency)
        .try_collect()
        .await
}

async fn wait_for_match_events(
    recorder: &Recorder,
    sockets: Vec<ClientSocket>,
    timeout: Duration,
    assignment_started: Instant,
) -> HarnessResult<usize> {
    if sockets.is_empty() {
        return Ok(0);
    }

    let events: Vec<()> = stream::iter(sockets)
        .map(|socket| {
            let recorder = recorder.clone();
            async move {
                let result = tokio_time::timeout(timeout, wait_for_match_event(socket)).await;
                match result {
                    Ok(Ok(())) => {
                        recorder.record(
                            SampleKind::Outcome,
                            "websocket_match_found",
                            assignment_started.elapsed(),
                            true,
                        );
                        Ok(())
                    }
                    Ok(Err(error)) => {
                        recorder.record(
                            SampleKind::Outcome,
                            "websocket_match_found",
                            assignment_started.elapsed(),
                            false,
                        );
                        Err(error)
                    }
                    Err(_) => {
                        recorder.record(
                            SampleKind::Outcome,
                            "websocket_match_found",
                            assignment_started.elapsed(),
                            false,
                        );
                        Err(harness_error("timed out waiting for match_found event"))
                    }
                }
            }
        })
        .buffer_unordered(64)
        .try_collect()
        .await?;

    Ok(events.len())
}

async fn wait_for_match_event(mut socket: ClientSocket) -> HarnessResult<()> {
    while let Some(message) = socket.next().await {
        match message? {
            Message::Text(text) => {
                let event: Value = serde_json::from_str(text.as_ref())?;
                if event.get("type").and_then(Value::as_str) == Some("match_found") {
                    let _ = socket.close(None).await;
                    return Ok(());
                }
            }
            Message::Ping(payload) => socket.send(Message::Pong(payload)).await?,
            Message::Close(_) => {
                return Err(harness_error(
                    "WebSocket closed before a match_found event arrived",
                ));
            }
            _ => {}
        }
    }

    Err(harness_error(
        "WebSocket ended before a match_found event arrived",
    ))
}

async fn request_json<T: DeserializeOwned>(
    recorder: &Recorder,
    operation: &'static str,
    request: RequestBuilder,
) -> HarnessResult<T> {
    let started = Instant::now();
    let response = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            recorder.record(SampleKind::Http, operation, started.elapsed(), false);
            return Err(Box::new(error));
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<unreadable body>".to_owned());
        recorder.record(SampleKind::Http, operation, started.elapsed(), false);
        return Err(harness_error(format!(
            "{operation} returned {status}: {body}"
        )));
    }

    match response.json::<T>().await {
        Ok(value) => {
            recorder.record(SampleKind::Http, operation, started.elapsed(), true);
            Ok(value)
        }
        Err(error) => {
            recorder.record(SampleKind::Http, operation, started.elapsed(), false);
            Err(Box::new(error))
        }
    }
}

async fn request_text(
    recorder: &Recorder,
    operation: &'static str,
    request: RequestBuilder,
) -> HarnessResult<String> {
    let started = Instant::now();
    let response = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            recorder.record(SampleKind::Http, operation, started.elapsed(), false);
            return Err(Box::new(error));
        }
    };
    let status = response.status();
    let body = response.text().await?;
    let success = status.is_success();
    recorder.record(SampleKind::Http, operation, started.elapsed(), success);

    if success {
        Ok(body)
    } else {
        Err(harness_error(format!(
            "{operation} returned {status}: {body}"
        )))
    }
}

fn deterministic_rating(seed: u64, index: usize, center: u16, spread: u16) -> u16 {
    let mut value = seed.wrapping_add((index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    value ^= value >> 30;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^= value >> 31;
    let width = u64::from(spread) * 2 + 1;
    let minimum = center - spread;
    minimum + u16::try_from(value % width).expect("rating width always fits in u16")
}

fn websocket_endpoint(base_url: &str, player_id: Uuid) -> HarnessResult<String> {
    let base = if let Some(value) = base_url.strip_prefix("http://") {
        format!("ws://{value}")
    } else if let Some(value) = base_url.strip_prefix("https://") {
        format!("wss://{value}")
    } else {
        return Err(harness_error("unsupported base_url scheme"));
    };

    Ok(format!(
        "{}/api/v1/ws?player_id={player_id}",
        base.trim_end_matches('/')
    ))
}

fn endpoint(base_url: &str, path: &str) -> String {
    format!("{}{}", base_url.trim_end_matches('/'), path)
}

fn percentile(sorted: &[u64], percentile: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (percentile * sorted.len()).div_ceil(100);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn duration_micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn micros_to_ms(micros: u64) -> f64 {
    micros as f64 / 1_000.0
}

fn harness_error(message: impl Into<String>) -> HarnessError {
    Box::new(io::Error::other(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_matchmaking_scenario() {
        let scenario: Scenario = serde_json::from_str(
            r#"{
                "name":"smoke",
                "base_url":"http://127.0.0.1:3000",
                "concurrency":4,
                "timeout_seconds":10,
                "workload":{
                    "kind":"matchmaking",
                    "players":4,
                    "mode":"2v2",
                    "connect_websockets":true,
                    "rating_seed":42,
                    "rating_center":1200,
                    "rating_spread":40,
                    "poll_interval_ms":50
                }
            }"#,
        )
        .expect("scenario should parse");

        assert!(scenario.validate().is_ok());
        assert_eq!(scenario.workload_name(), "matchmaking");
    }

    #[test]
    fn rejects_incomplete_matchmaking_group() {
        let scenario = Scenario {
            name: "bad".to_owned(),
            base_url: "http://127.0.0.1:3000".to_owned(),
            concurrency: 1,
            timeout_seconds: 10,
            workload: Workload::Matchmaking {
                players: 3,
                mode: MatchMode::TwoVsTwo,
                connect_websockets: false,
                rating_seed: 1,
                rating_center: 1200,
                rating_spread: 20,
                poll_interval_ms: 50,
            },
        };

        assert!(scenario.validate().is_err());
    }

    #[test]
    fn deterministic_ratings_are_stable_and_bounded() {
        let first: Vec<u16> = (0..100)
            .map(|index| deterministic_rating(42, index, 1200, 40))
            .collect();
        let second: Vec<u16> = (0..100)
            .map(|index| deterministic_rating(42, index, 1200, 40))
            .collect();

        assert_eq!(first, second);
        assert!(first.iter().all(|rating| (1160..=1240).contains(rating)));
    }

    #[test]
    fn percentile_uses_nearest_rank() {
        let values = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

        assert_eq!(percentile(&values, 50), 5);
        assert_eq!(percentile(&values, 95), 10);
        assert_eq!(percentile(&[], 95), 0);
    }

    #[test]
    fn parses_cli_arguments() {
        let arguments = Arguments::parse([
            "--scenario".to_owned(),
            "load/scenarios/smoke.json".to_owned(),
            "--output".to_owned(),
            "target/report.json".to_owned(),
        ])
        .expect("arguments should parse");

        assert_eq!(
            arguments.scenario,
            Some(PathBuf::from("load/scenarios/smoke.json"))
        );
        assert_eq!(arguments.output, Some(PathBuf::from("target/report.json")));
    }
}
