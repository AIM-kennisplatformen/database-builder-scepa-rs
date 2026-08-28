# syntax=docker/dockerfile:1

FROM rust:1.97-trixie AS development

RUN cargo install cargo-watch --locked

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo fetch --locked

EXPOSE 8002
CMD ["cargo", "watch", "--watch", "src", "--watch", "Cargo.toml", "--watch", "Cargo.lock", "--exec", "run"]

FROM rust:1.97-trixie AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo build --locked --release \
    && cp target/release/scepa-mcp /usr/local/bin/scepa-mcp

FROM debian:trixie-slim AS release

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/bin/scepa-mcp /usr/local/bin/scepa-mcp

EXPOSE 8002
ENTRYPOINT ["scepa-mcp"]
