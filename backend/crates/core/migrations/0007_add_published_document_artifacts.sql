ALTER TABLE document_artifacts
    ADD COLUMN IF NOT EXISTS published_artifact jsonb,
    ADD COLUMN IF NOT EXISTS published_at timestamptz;

CREATE INDEX IF NOT EXISTS document_artifacts_published_at_idx
    ON document_artifacts (published_at DESC)
    WHERE published_artifact IS NOT NULL;
