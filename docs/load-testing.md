# ArenaCore Load Testing and Profiling

The `arena-load` binary drives the deployed HTTP and WebSocket interfaces. It uses deterministic rating generation, bounded concurrency, and JSON scenario files so the same workload can be repeated locally or in CI.

## Start a load-test deployment

Use the Compose override to raise the API rate limit for intentional test traffic:

```powershell
docker compose -f compose.yaml -f compose.load.yaml up --build -d
docker compose -f compose.yaml -f compose.load.yaml ps
```

The override changes only the load-test API container. Do not apply it to an internet-facing environment.

## Run scenarios

Build and run the harness in release mode:

```powershell
cargo run --release --bin arena-load -- --scenario load/scenarios/smoke.json --output target/load-reports/smoke.json
cargo run --release --bin arena-load -- --scenario load/scenarios/health.json --output target/load-reports/health.json
cargo run --release --bin arena-load -- --scenario load/scenarios/matchmaking-1v1.json --output target/load-reports/matchmaking-1v1.json
cargo run --release --bin arena-load -- --scenario load/scenarios/matchmaking-2v2.json --output target/load-reports/matchmaking-2v2.json
```

The smoke scenario creates four players, binds four development sessions, opens four WebSockets, forms one 2v2 match, verifies every assignment through HTTP, and requires four `match_found` events.

The health scenario measures the HTTP stack without database or matchmaking work. The 1v1 and 2v2 scenarios measure player creation, enqueue latency, assignment visibility, total matches formed, and overall throughput against PostgreSQL.

## Scenario fields

Common fields:

- `base_url`: deployed ArenaCore URL.
- `concurrency`: maximum in-flight work created by the harness.
- `timeout_seconds`: request and outcome deadline.
- `workload.kind`: `health` or `matchmaking`.

Matchmaking fields:

- `players`: must be divisible by two for `1v1` or four for `2v2`.
- `connect_websockets`: verifies authenticated event delivery when enabled.
- `rating_seed`: produces the same ratings on repeated runs.
- `rating_center` and `rating_spread`: keep generated ratings within a controlled compatibility window.
- `poll_interval_ms`: delay between authoritative status reads while waiting for assignment.

## Report interpretation

The JSON report includes:

- total HTTP requests and WebSocket handshakes;
- failures and successfully formed matches;
- observed `match_found` events;
- overall HTTP throughput;
- minimum, mean, p50, p95, p99, and maximum latency by operation;
- time-to-match samples measured after the enqueue phase.

Run a warm-up once before recording results. Keep the machine, Docker resource limits, database state, scenario file, and build profile unchanged when comparing commits. Treat results from a developer laptop as regression indicators, not production capacity promises.

## Reproducible benchmark procedure

1. Start an otherwise idle Docker Desktop instance.
2. Start the Compose stack with the load override and wait for both services to become healthy.
3. Run the smoke scenario as a correctness gate.
4. Run the chosen benchmark once as warm-up.
5. Run it at least five measured times, storing every JSON report.
6. Compare medians and p95/p99 latency rather than selecting the fastest run.
7. Record CPU allocation, Docker memory limits, Rust version, commit SHA, and scenario file with the results.
8. Inspect `/metrics` before and after each run for unexpected 4xx, 5xx, or rate-limit counters.

Use a disposable database for destructive test automation. The harness only creates new records, but repeated runs intentionally accumulate players, matches, and history.

## CPU profiling

The custom Cargo `profiling` profile retains symbols while preserving release optimizations:

```powershell
cargo build --profile profiling --bin arena-core
```

On Windows, record `target\profiling\arena-core.exe` with Windows Performance Recorder or another sampling profiler while the health and matchmaking scenarios run. On Linux, use the same build with `perf record -g` and inspect the resulting call stacks with your preferred `perf` visualization.

Profile one workload at a time. The health scenario isolates routing, middleware, serialization, and socket overhead. The matchmaking scenarios add repository queries, PostgreSQL transaction locks, selection, and team balancing. A useful investigation starts at the operation with a regressed p95, then correlates it with CPU stacks, PostgreSQL activity, and ArenaCore metrics.

## Database investigation

For a slow PostgreSQL-backed run:

- inspect active and waiting sessions in `pg_stat_activity`;
- look for time spent waiting on the matchmaking advisory transaction lock;
- use `EXPLAIN (ANALYZE, BUFFERS)` on a representative query in a disposable environment;
- verify table and index statistics are current;
- separate database time from application time before changing the matching algorithm.

The current global advisory lock intentionally favors correct cross-worker matching over maximum queue throughput. Load results should establish whether a later partitioned queue design is necessary.
