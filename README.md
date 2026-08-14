# SCEPA

SCEPA runs PDF extraction and TEI conversion as a durable Restate workflow.
Garage stores immutable source PDFs by SHA-256, PostgreSQL indexes those
objects, and Axum exposes an API for uploading new documents.

## Repository layout

```text
backend/                 Rust workspace
  crates/api/            Axum API and Restate server
  crates/cli/            Clap command-line client
  crates/core/           Shared pipeline, models, and persistence
frontend/                Reserved frontend workspace
compose.yaml             Local application and infrastructure stack
```

The backend is self-contained: its Cargo manifest, lockfile, Dockerfile, and
all Rust crates live under `backend/`. This keeps its dependency and build
configuration independent from the frontend toolchain that will be added under
`frontend/`.

Run backend development commands from its workspace:

```bash
cd backend
cargo check --workspace
cargo test --workspace
cargo run --package scepa-api
cargo run --package scepa-cli -- --help
```

## Start the local stack

```bash
docker compose up --build
```

The stack exposes:

- Axum API: `http://localhost:3000`
- Restate ingress: `http://localhost:8080`
- Restate admin UI/API: `http://localhost:9070`
- Restate service discovery endpoint: `http://localhost:9080`
- Grobid: `http://localhost:8070`
- Garage S3 API: `http://localhost:3900`
- Garage admin API: `http://localhost:3903`
- PostgreSQL: `localhost:5432`
- TypeDB gRPC: `localhost:1729`
- TypeDB HTTP: `http://localhost:8000`

The `restate-register` Compose service registers the application endpoint with
Restate after both services have started.

## API

Upload a PDF as the first step of the pipeline:

```bash
curl --request POST \
  --header 'content-type: application/pdf' \
  --data-binary @paper.pdf \
  http://localhost:3000/pdfs
```

The response contains the lowercase SHA-256 hash, Garage object key, and
hash-keyed workflow ID. The PDF is written to Garage before its PostgreSQL
index row and workflow submission are created. This is the API's only route.

## Pipeline CLI

The CLI exposes exactly two pipeline commands. With the Compose services
running, use the same `DATABASE_URL` and `RESTATE_INGRESS_URL` environment
variables as the server. Workflow identifiers are derived from file stems:

```bash
scepa-cli single paper.pdf
scepa-cli batch ./papers
```

Use `--identifier` to rerun a PDF under a fresh Restate workflow key:

```bash
scepa-cli single --identifier 2AEJBJL6-debug .sources/pdfs/2AEJBJL6.pdf
```

The `scepa-api` binary only runs the HTTP and Restate endpoints; all command-line
operations live in the separate `scepa-cli` crate.

## Debug artifacts

Successful workflow executions retain both intermediate representations:

```text
.artifacts/tei/{workflow_id}.tei.xml
.artifacts/json/{workflow_id}.json
```

The JSON file is the pretty-printed typed `TeiDocument`. Debug logging is
enabled by default in Compose with `scepa=debug`. For a locally started API
server, use `RUST_LOG=scepa=debug`; change the output root with
`DEBUG_ARTIFACT_ROOT`.
