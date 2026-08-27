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
publication writes canonical metadata to TypeDB and source-plus-combined passage
embeddings to Qdrant before finalizing the artifact. Combined points contain only whole
source passages, target 500 estimated tokens, normally stop at 800, and use
whole-passage overlap near 80 tokens. This keeps every stored PDF bounding box
on its source point; combined/source reference arrays use Qdrant point UUIDs.
Payload indexes are created for `is_abstract`, `is_combined`, and `pdf_hash`.
This schema requires recreating the collection and republishing documents; no
backfill is performed. A single oversized passage is kept whole.
`EMBEDDING_MAX_CONCURRENCY` limits the number of process-wide embedding
HTTP calls (default `4`). The CLI sends
uploads to `SCEPA_API_URL` (default `http://localhost:3000`).

## Database import and export

TypeDB Console must be installed and the `typedb` executable must be available
on `PATH`. Export a database with a non-interactive console command:

```bash
mkdir -p typedb-backup
typedb console \
  --address localhost:1729 \
  --username admin \
  --password password \
  --tls-disabled \
  --command="database export scepa typedb-backup/schema.typeql typedb-backup/data.typedb"
```

Import it with:

```bash
typedb console \
  --address localhost:1729 \
  --username admin \
  --password password \
  --tls-disabled \
  --command="database import scepa typedb-backup/schema.typeql typedb-backup/data.typedb"
```

The target database must not already exist when importing.

Migrate a local Qdrant collection to Qdrant Cloud with:

```bash
docker run --net=host registry.cloud.qdrant.io/library/qdrant-migration \
  --source-url http://localhost:6333 \
  --target-url https://your-cloud-cluster.qdrant.io \
  --target-api-key "your-api-key" \
  --collection my_collection
```

Replace the target URL, API key, and collection name for the destination.

Runtime services and environment variables are managed from the repository
root with `docker compose` and `.env`.
