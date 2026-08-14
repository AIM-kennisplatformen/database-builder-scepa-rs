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

Runtime services and environment variables are managed from the repository
root with `docker compose` and `.env`.
