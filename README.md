# SCEPA

SCEPA contains the PDF extraction, TEI conversion, storage, API, and CLI
building blocks for a document pipeline. Uploads are stored in Garage before
the durable `NewDocumentWorkflow` is invoked with the PDF's content hash.

## Repository layout

```text
backend/                 Rust workspace
  crates/api/            Axum API
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
- Grobid: `http://localhost:8070`
- Garage S3 API: `http://localhost:3900`
- Garage admin API: `http://localhost:3903`
- PostgreSQL: `localhost:5432`
- Restate ingress: `http://localhost:8080`
- Restate admin UI/API: `http://localhost:9070`
- TypeDB gRPC: `localhost:1729`
- TypeDB HTTP: `http://localhost:8000`

## API

`POST /pdfs` submits a PDF to `NewDocumentWorkflow`, using its SHA-256 hash as
the workflow identifier, and waits for the workflow result.

```bash
curl --request POST \
  --header 'content-type: application/pdf' \
  --data-binary @paper.pdf \
  http://localhost:3000/pdfs
```

`POST /pdfs/submissions/{workflow_id}` stores the PDF and starts the workflow
without waiting for its result. The CLI uses this asynchronous route.

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
