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
mcp/                     Standalone literature retrieval MCP project
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
- React UI: `http://localhost:5173/upload/`
- Grobid: `http://localhost:8070`
- Garage S3 API: `http://localhost:3900`
- Garage admin API: `http://localhost:3903`
- PostgreSQL: `localhost:5432`
- Restate ingress: `http://localhost:8080`
- Restate admin UI/API: `http://localhost:9070`
- TypeDB gRPC: `localhost:1729`
- TypeDB HTTP: `http://localhost:8000`
- TypeDB MCP with `tools`: `http://localhost:8001`
- SCEPA literature MCP: `http://localhost:8002/mcp`
- Qdrant HTTP/gRPC: `localhost:6333` / `localhost:6334`
- SonarQube with `tools`: `http://localhost:9000`

## Release deployment and administrative access

`compose.release.yaml` builds the `release` Dockerfile stages for the API,
frontend, and MCP services. All published ports bind to `127.0.0.1` in release
mode. This keeps the services reachable from the deployment host while
preventing direct access through the VM's public network interfaces.

The frontend and MCP loopback ports are intended as upstreams for a reverse
proxy such as Caddy, which should provide public HTTPS on ports 80 and 443. The
remaining ports are for administrators and should not be changed to
`0.0.0.0`. Access them remotely through SSH tunnels instead.

| Service | Local endpoint |
| --- | --- |
| Frontend, including the `/upload/api/` proxy | `http://127.0.0.1:5173/upload/` |
| API, for direct diagnostics | `http://127.0.0.1:3000` |
| Literature MCP | `http://127.0.0.1:8002/mcp` |
| TypeDB gRPC | `127.0.0.1:1729` |
| TypeDB HTTP | `http://127.0.0.1:8000` |
| PostgreSQL | `127.0.0.1:5432` |
| Qdrant HTTP | `http://127.0.0.1:6333` |
| Qdrant gRPC | `127.0.0.1:6334` |
| Garage S3 API | `http://127.0.0.1:3900` |
| Garage admin API | `http://127.0.0.1:3903` |
| Grobid | `http://127.0.0.1:8070` |
| Restate ingress | `http://127.0.0.1:8080` |
| Restate admin UI/API | `http://127.0.0.1:9070` |
| Restate fabric | `127.0.0.1:5122` |

To access one service from an administrator workstation, forward its port over
SSH. For example, this exposes the Restate admin UI at
`http://localhost:9070` on the workstation:

```bash
ssh -N -L 9070:127.0.0.1:9070 user@your-vm
```

Multiple services can be forwarded in one session. This example provides local
access to Restate, Qdrant, TypeDB HTTP, Garage admin, and PostgreSQL:

```bash
ssh -N \
  -L 9070:127.0.0.1:9070 \
  -L 6333:127.0.0.1:6333 \
  -L 8000:127.0.0.1:8000 \
  -L 3903:127.0.0.1:3903 \
  -L 5432:127.0.0.1:5432 \
  user@your-vm
```

Keep the SSH session open while using the forwarded services. Replace
`user@your-vm` with the deployment account and VM hostname. If a workstation
port is already occupied, change only the first port in that forwarding rule;
for example, `-L 15432:127.0.0.1:5432` makes PostgreSQL available locally on
port `15432`.

Port variables in `.env` change both the VM loopback port and the corresponding
SSH-tunnel source port. Inspect the effective release configuration before
deployment with:

```bash
docker compose -f compose.release.yaml config
```

### Single-host HTTPS routing

The frontend uses `/upload/` as its base path in development and release builds.
Local development serves the upload page at
`http://localhost:5173/upload/`; a deployment preserves the same path at
`https://scepakp.mads-han.src.surf-hosted.nl/upload/`.

The release Nginx container accepts `/upload/` directly and proxies
`/upload/api/` to the API container. Caddy must therefore preserve the prefix
rather than stripping it:

```caddy
scepakp.mads-han.src.surf-hosted.nl {
    encode zstd gzip

    handle /mcp* {
        reverse_proxy 127.0.0.1:8002 {
            flush_interval -1
        }
    }

    redir /upload /upload/ 308

    handle /upload/* {
        basic_auth {
            operator REPLACE_WITH_CADDY_PASSWORD_HASH
        }

        reverse_proxy 127.0.0.1:5173
    }

    redir /chatep /chatep/ 308

    handle /chatep/* {
        reverse_proxy 127.0.0.1:10090
    }

    handle {
        respond "Not found" 404
    }
}
```

This serves Studio and its API and Socket.IO routes under `/chatep/`, serves
the bearer-token-protected literature MCP at `/mcp`, and keeps the SCEPA
operator UI and its API under the Basic-authenticated `/upload/` path.

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
error, artifact metadata, and retryability. Each case also includes the document
summary fields used by the document picker: `title` from its effective repair
draft (or published artifact when no draft exists), plus `published_at` when
it has previously been published. Missing values are `null`. `GET` and `PUT` on
`/documents/requiring-fixing/{case_id}` load a repair draft and submit manually
fixed data through `UpdateDocumentWorkflow`, respectively. External enrichment
is represented by the repair contract but intentionally returns `501` until an
enrichment service exists.

The shared edit and repair form can manually classify a document for any of the
`strategic_overview`, `best_practices`, and `target_groups` personas and as
`grey_literature`, `scientific_literature`, or `project_report`. Classification
is optional and is never inferred during PDF ingestion.

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

## Literature MCP

The self-contained project under `mcp/` exposes authenticated Streamable HTTP at
`/mcp`. `search_literature` first obtains eligible PDF hashes from TypeDB using
publication-date, document-type, classification, and organization filters, similarity-searches
`4 × top_k` source passages in Qdrant, resolves their linked combined passages,
and reranks them locally. Search responses always include bibliographic metadata
as deterministic IEEE references with passages grouped by document. PDF hashes
and reranker scores remain internal and are not returned by the MCP tool.

Set `MCP_BEARER_TOKEN` before starting the service. The unquantized
`cross-encoder/ms-marco-MiniLM-L6-v2` ONNX model is downloaded on first startup
and retained in the `mcp_model_cache` volume. The model ID, revision, batch size,
and other service settings are configurable through the variables in
`.env.example`. The search tool's optional `top_k` parameter defaults to 30 and
accepts values from 1 through 50. See `mcp/README.md` for standalone setup and
deployment instructions.
