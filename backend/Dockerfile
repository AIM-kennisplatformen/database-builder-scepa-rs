# syntax=docker/dockerfile:1

FROM lukemathwalker/cargo-chef:0.1.78-rust-1.97-bookworm AS chef

WORKDIR /app

FROM chef AS planner

COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS development

RUN cargo install cargo-watch --locked

COPY --from=planner /app/recipe.json recipe.json

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo chef cook --locked --package scepa-api --recipe-path recipe.json

COPY . .

EXPOSE 3000 9080

CMD ["cargo", "watch", "--watch", "crates", "--watch", "Cargo.toml", "--watch", "Cargo.lock", "--exec", "run --package scepa-api"]

FROM chef AS builder

COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    cargo chef cook --locked --release --package scepa-api --recipe-path recipe.json

COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    cargo build --locked --release --package scepa-api \
    && cp target/release/scepa-api /usr/local/bin/scepa-api

FROM debian:bookworm-slim AS release

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/bin/scepa-api /usr/local/bin/scepa-api

EXPOSE 3000 9080
ENTRYPOINT ["scepa-api"]
