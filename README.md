# SCEPA

SCEPA contains the PDF extraction, TEI conversion, storage, vector publication, API, and CLI
building blocks for a document pipeline. Uploads are stored in Garage before
the durable `NewDocumentWorkflow` is invoked with the PDF's content hash.

## Repository layout

```text
backend/                 Rust workspace
  crates/api/            Axum API
  crates/cli/            Clap command-line client
  crates/core/           Shared pipeline, models, and persistence
frontend/                React operator UI
compose.yaml             Default development stack with hot reloading
compose.development.yaml Development tools extension
compose.release.yaml     Release build overrides
```

The backend is self-contained: its Cargo manifest, lockfile, Dockerfile, and
all Rust crates live under `backend/`. This keeps its dependency and build
configuration independent from the frontend toolchain under `frontend/`.

HTTP handlers are transport adapters only. Uploads store PDF bytes in Garage
before the API invokes `NewDocumentWorkflow` with the resulting hash. Other
write endpoints invoke their typed workflow through the shared `RestateClient`;
read endpoints query the relevant core persistence adapter directly.

Run backend development commands from its workspace:

```bash
cd backend
cargo check --workspace
cargo test --workspace
cargo run --package scepa-api
cargo run --package scepa-cli -- --help
```

## Start the local stack

For development builds with backend and frontend hot reloading:

```bash
docker compose up --build
```

Start the optional TypeDB MCP and SonarQube tools with the `tools` profile:

```bash
docker compose -f compose.development.yaml --profile tools up --build
```

For optimized release builds:

```bash
docker compose -f compose.release.yaml up --build
```

The stack exposes:

- Axum API: `http://localhost:3000`
- React UI: `http://localhost:5173`
- Grobid: `http://localhost:8070`
- Garage S3 API: `http://localhost:3900`
- Garage admin API: `http://localhost:3903`
- PostgreSQL: `localhost:5432`
- Restate ingress: `http://localhost:8080`
- Restate admin UI/API: `http://localhost:9070`
- TypeDB gRPC: `localhost:1729`
- TypeDB HTTP: `http://localhost:8000`
- TypeDB MCP with `tools`: `http://localhost:8001`
- Qdrant HTTP/gRPC: `localhost:6333` / `localhost:6334`
- SonarQube with `tools`: `http://localhost:9000`

## API

The API generates an OpenAPI 3.1 document from its handler annotations. With
the service running, download it from `http://localhost:3000/openapi.json` for
client generation, validation, or documentation tooling, or browse the
interactive Swagger UI at `http://localhost:3000/swagger-ui/`.

`POST /pdfs` submits a PDF to `NewDocumentWorkflow`, using its SHA-256 hash as
the workflow identifier, and waits for extraction, TypeDB export, embedding and
Qdrant publication, and valid artifact persistence. The returned artifact then
opens in the shared update flow for optional manual corrections.

Every non-empty effective abstract and body passage is embedded through the
OpenAI-compatible endpoint configured by `OPENAI_HOST`, `OPENAI_API_KEY`, and
`OPENAI_EMBEDDING_MODEL`. The same publication also creates combined vectors from
complete adjacent passages, targeting 500 estimated tokens, stopping at 800,
and reusing up to 100 tokens of complete trailing passages around the 80-token
overlap target. Section and heading changes are hard boundaries. An individual
source passage over 800 tokens remains whole so its PDF coordinates are never
assigned to text outside that passage.

Source and combined embedding inputs are prefixed with the available document
title, section, and heading. Source Qdrant payloads contain `id`, `pdf_hash`,
unprefixed `text`, `combined_point_ids`, `is_abstract`, `is_combined: false`,
`bounding_boxes`, and optional `section` and `heading`. Combined payloads contain
the same identity, text, marker, and optional context fields, with
`is_combined: true` and `source_point_ids` instead of bounding boxes. Both
reference arrays contain Qdrant point UUIDs. Qdrant creates boolean payload
indexes for `is_abstract` and `is_combined` and a keyword index for `pdf_hash`.

Updating a document refreshes its complete source-and-combined vector set. This
payload contract is a breaking change: recreate the Qdrant collection and
republish documents when deploying it; there is no historical backfill. Set
`EMBEDDING_MAX_CONCURRENCY` (default `4`) to cap embedding HTTP calls across all
workflows in one API process.

```bash
curl --request POST \
  --header 'content-type: application/pdf' \
  --data-binary @paper.pdf \
  http://localhost:3000/pdfs
```

`POST /pdfs/submissions/{workflow_id}` stores the PDF and starts the workflow
without waiting for its result. The CLI uses this asynchronous route. A `202`
response confirms durable acceptance; successful CLI submissions publish to
TypeDB automatically, while extraction and canonical-validation failures are
retained for operator repair.

`GET /documents/requiring-fixing` returns every pending review case, newest
first, including its document hash (when available), failed pipeline phase,
error, artifact metadata, and retryability. `GET` and `PUT` on
`/documents/requiring-fixing/{case_id}` load a repair draft and submit manually
fixed data through `UpdateDocumentWorkflow`, respectively. External enrichment
is represented by the repair contract but intentionally returns `501` until an
enrichment service exists.

## Pipeline CLI

The CLI sends uploads to `SCEPA_API_URL` (default `http://localhost:3000`) and
provides single-file and directory upload commands:

```bash
scepa-cli single paper.pdf
scepa-cli batch ./papers
```

The single-file command also accepts an explicit identifier:

```bash
scepa-cli single --identifier 2AEJBJL6-debug .sources/pdfs/2AEJBJL6.pdf
```

The `scepa-api` binary only runs the HTTP endpoint; command-line operations live
in the separate `scepa-cli` crate.
