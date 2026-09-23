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

The API searches for a `.env` file before reading its configuration. To load a
specific file instead, pass `--env-file <PATH>`:

```bash
cargo run --package scepa-api -- --env-file ../.env
```

Missing dotenv files are ignored so deployments can provide configuration
through process environment variables. Existing process variables take
precedence over values from the dotenv file.

Every API environment setting is also available as a command-line option. Run
`cargo run --package scepa-api -- --help` for the complete configuration and
its environment variable names. Command-line values take precedence over the
environment and dotenv defaults.

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

Draft responses contain deterministic UUIDv5 identifiers for documents,
contributors, organizations, venues, passages, media, and graph
relations. The identifiers are scoped to the PDF and remain unchanged when an
object's metadata or list position changes. Update requests preserve IDs for
existing objects. When a `PUT` creates an object without an ID, it must include
an `Idempotency-Key` header; retrying with the same key generates the same IDs.
Publisher, journal, and affiliation values are represented as nested objects
instead of plain strings.

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

## Repeating document operations

Each ordinary `POST /pdfs` and document/draft/repair `PUT` starts a fresh
Restate workflow, even when the arguments are identical. Upload responses return
an opaque, unique `workflow_id`; use `pdf_hash` to identify the stored document.
Retries inside that invocation retain the same parent and child workflow IDs.
Repeated HTTP requests create independent work and do not guarantee ordering
when submitted concurrently.

Re-uploading a published PDF runs extraction again, preserves manual corrections
from its current draft (or published artifact if no draft exists), and updates
the existing graph and vectors. Repairs accept pending and resolved cases;
repeating a resolved repair leaves its original resolution metadata intact.
The review list and review GET endpoint still expose only pending cases.

`POST /pdfs/submissions/{workflow_id}` and the CLI retain caller-selected IDs.
Use a different identifier to start another explicitly named submission.

Identity conflicts return HTTP `409` with the existing `{"error":"…"}` body:
workflow/PDF mismatches, duplicate records, canonical key/uniqueness conflicts,
and duplicate passage identities. These are terminal workflow failures, not
transient errors to retry. Synchronous uploads whose extracted drafts are missing
canonical fields return `422` with the pending review-case ID and all missing
field paths. Other upstream failures continue to return `502`.
An accepted asynchronous submission can still fail later; its `202` response
only confirms acceptance, not successful publication.

Before deploying these changed workflow sequences, drain active Restate
invocations. Existing completed workflow history needs no deletion or migration.
