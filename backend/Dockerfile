# syntax=docker/dockerfile:1

FROM rust:1.97-bookworm AS development

RUN cargo install cargo-watch --locked

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

EXPOSE 3000 9080
CMD ["cargo", "watch", "--watch", "crates", "--watch", "Cargo.toml", "--watch", "Cargo.lock", "--exec", "run --package scepa-api"]

FROM rust:1.97-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo build --locked --release --package scepa-api \
    && cp target/release/scepa-api /usr/local/bin/scepa-api

FROM debian:bookworm-slim AS release

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/bin/scepa-api /usr/local/bin/scepa-api

EXPOSE 3000 9080
ENTRYPOINT ["scepa-api"]
