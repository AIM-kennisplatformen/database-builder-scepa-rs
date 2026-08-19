# SCEPA backend

This directory is the complete Rust backend workspace.

```text
crates/api/    HTTP API (`scepa-api`)
crates/cli/    Operator command-line interface (`scepa-cli`)
crates/core/   Shared domain, pipeline, and storage code (`scepa`)
```

Common commands:

```bash
cargo check --workspace
cargo test --workspace
cargo run --package scepa-api
cargo run --package scepa-cli -- --help
```

The API reads `RESTATE_INGRESS_URL` (default `http://localhost:8080`) and
invokes `NewDocumentWorkflow` after storing each upload in Garage. Successful
publication writes canonical metadata to TypeDB and passage embeddings to
Qdrant before finalizing the artifact. `EMBEDDING_MAX_CONCURRENCY` limits the
number of process-wide embedding HTTP calls (default `4`). The CLI sends
uploads to `SCEPA_API_URL` (default `http://localhost:3000`).

Runtime services and environment variables are managed from the repository
root with `docker compose` and `.env`.
