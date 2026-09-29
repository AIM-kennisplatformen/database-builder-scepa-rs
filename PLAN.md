# Add Manual Document Classifications

## Summary

Add the classifications from [issue #31](https://github.com/AIM-kennisplatformen/database-builder-scepa-rs/issues/31) to the edit/fix workflow:

- User personas: zero to three of `strategic_overview`, `best_practices`, and `target_groups`.
- Literature kind: optionally one of `grey_literature`, `scientific_literature`, and `project_report`.
- Keep PDF upload endpoints unchanged. Newly uploaded documents may remain unclassified.
- Do not import or backfill values from the v2 deployment.

## Implementation Changes

- Add shared Rust enums and a `DocumentClassification` model containing unique `user_personas` and optional `literature_kind`. Store it under `ManualDocument.classification`, defaulting safely when older JSON artifacts lack the field.
- Return classification data with existing draft/published artifact responses. Reject duplicate or unsupported persona values with HTTP 422 on draft publication, document update, and repair requests; empty classifications remain valid.
- Add the classification to every canonical document variant and persist selected values as boolean TypeDB marker attributes owned by each document:
  - Abstract `classification` attribute constrained to the value `true`.
  - Abstract `user_persona` branch with `strategic_overview`, `best_practices`, and `target_groups` leaf attributes.
  - Abstract `kind_of_literature` branch with `grey_literature`, `scientific_literature`, and `project_report` leaf attributes.
  - Keep all marker attributes optional for compatibility with existing and newly uploaded unclassified documents.
- Extend schema verification and document insert/update queries so classification changes replace the stored attributes.
- Add a classification section to the shared edit/fix page:
  - Three persona checkboxes.
  - An optional literature-kind select with an unselected placeholder.
  - Load saved values, preserve them during other edits, and show validation before submitting.
- Extend `search_literature` with optional `user_personas` and `literature_kinds` arrays. Values within either array use OR semantics; classification categories combine with each other and existing filters using AND. Empty arrays impose no restriction. MCP response fields remain unchanged.
- Update API/OpenAPI and MCP documentation with the enum values and filtering behavior.

## Test Plan

- [x] Verify old artifacts without `classification` still deserialize and remain readable.
- [x] Test valid optional classifications, duplicate personas, and unsupported values.
- [x] Test canonical conversion and TypeDB marker attributes for empty, single, and multiple persona selections.
- [x] Test schema installation/migration against a database containing unclassified documents.
- [x] Test MCP query generation for each classification filter and combinations with date, document-type, and organization filters; verify the tool schema describes every new argument.
- [x] Build the frontend and run both Rust workspaces’ unit tests, OpenAPI assertions, and available live TypeDB schema tests.

## Assumptions

- The “upload interface” is the edit/fix form reached immediately after uploading, rather than the initial PDF picker.
- Raw PDF upload and CLI behavior stay unchanged.
- Unclassified documents remain eligible for unfiltered MCP searches; classification filters exclude documents without matching values.
- Classification is manual only and is never inferred from GROBID or other extraction output.
