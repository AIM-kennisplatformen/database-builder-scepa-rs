# SCEPA

SCEPA runs PDF extraction and TEI conversion as a durable Restate workflow.
Garage stores immutable source PDFs by SHA-256, PostgreSQL indexes those
objects and stores review cases, and Axum exposes the public and operator APIs.

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
index row and workflow submission are created. Download it again through the
PostgreSQL index with:

```bash
curl --output paper.pdf \
  http://localhost:3000/pdfs/{sha256}
```

Submit a PDF using a stable workflow ID:

```bash
curl --request POST \
  --header 'content-type: application/pdf' \
  --data-binary @paper.pdf \
  http://localhost:3000/workflows/paper-123
```

Submission is asynchronous. Read the workflow output with:

```bash
curl http://localhost:3000/workflows/paper-123/output
```

The stable-ID endpoint uses the same Garage-first ingestion path. Restate
receives only the PDF hash and loads the bytes through PostgreSQL and Garage.
The same SHA-256 is stored as the TypeDB document's `pdf_hash` attribute.

Review endpoints:

```text
GET  /review-cases?status=pending&limit=100
GET  /review-cases/count
GET  /review-cases/{id}
GET  /review-cases/{id}/artifact
POST /review-cases/{id}/resolve
```

Review-case metadata includes `pdf_hash` when the workflow has a stored source
PDF. Download that source with:

```bash
curl --fail --show-error --output paper.pdf \
  http://localhost:3000/pdfs/<pdf_hash>
```

Get the number of cases currently waiting for human review:

```bash
curl http://localhost:3000/review-cases/count
```

Resolve a retryable Grobid processing failure and resume its suspended workflow:

```bash
curl --request POST \
  --header 'content-type: application/json' \
  --data '{"decision":"retry"}' \
  http://localhost:3000/review-cases/1/resolve
```

Use `{"decision":"abort"}` to terminate the suspended workflow instead.

The health endpoint is `GET /healthz`.

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

## Pipeline API

The matching HTTP operations are:

```text
POST  /pipeline/grobid/{input-validation|output-validation|execute}/{review_case_id}
PATCH /pipeline/grobid/{input-validation|output-validation}/{review_case_id}
POST  /pipeline/run/{workflow_id}                 application/pdf body
POST  /pipeline/run/batch                         multipart PDF fields
DELETE /pipeline/artifacts/{workflow_id}
PATCH /review-cases/{review_case_id}/artifact     replacement artifact body
```

Deleting a pipeline's artifacts purges the retained Restate workflow, deletes
its PostgreSQL review cases and removes generated TEI XML and JSON files. The
content-addressed source PDF and its PostgreSQL index remain available through
`GET /pdfs/{sha256}`. The operation is idempotent. An active workflow is killed
before it is purged.

Set the replacement artifact's media type with the `Content-Type` header.
Artifact patches are accepted only while an input- or output-validation case is
pending; processing artifacts are immutable. The validation-specific PATCH
routes replace the artifact and immediately rerun that validation phase. The
review-case artifact route only performs the replacement.
