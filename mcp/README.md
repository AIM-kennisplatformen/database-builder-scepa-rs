# SCEPA Literature MCP

This directory is a self-contained Rust project and can be moved to its own
repository without any backend source dependency.

The server exposes:

- `GET /health`, without authentication.
- Streamable HTTP MCP at `/mcp`, protected by a static bearer token.
- `search_literature`, which filters document hashes in TypeDB, searches source
  passages in Qdrant, expands their linked combined passages, and reranks them
  with a local ONNX cross-encoder.
- `get_document_metadata`, which resolves one or more opaque `pdf_hash` handles
  to TypeDB metadata.

## Run locally

Copy `.env.example` to your preferred environment configuration, provide the
required secrets and service endpoints, then:

```sh
cargo run --release
```

The default listen address is `0.0.0.0:8002`. The reranking model is downloaded
from Hugging Face during the first startup and cached according to `HF_HOME`.

## Build and test

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --locked --release
```

To build the standalone production image:

```sh
docker build --target release -t scepa-mcp .
```
