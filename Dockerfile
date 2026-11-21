FROM rust:1.100-slim-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY migrations ./migrations
COPY src ./src
COPY tests ./tests
RUN cargo build --locked --release

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install --no-install-recommends -y ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 arena

COPY --from=builder /app/target/release/arena-core /usr/local/bin/arena-core

USER arena
EXPOSE 3000
ENTRYPOINT ["/usr/local/bin/arena-core"]
