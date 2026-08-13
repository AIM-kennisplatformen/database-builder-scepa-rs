CREATE TABLE IF NOT EXISTS review_cases (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    workflow_id text NOT NULL,
    service text NOT NULL,
    phase text NOT NULL CHECK (
        phase IN ('input_validation', 'processing', 'output_validation')
    ),
    error_message text NOT NULL,
    artifact_content_type text NOT NULL,
    artifact_bytes bytea NOT NULL,
    status text NOT NULL DEFAULT 'pending' CHECK (
        status IN ('pending', 'retry_requested', 'skipped', 'resolved', 'aborted')
    ),
    resolution jsonb,
    created_at timestamptz NOT NULL DEFAULT now(),
    resolved_at timestamptz,
    UNIQUE (workflow_id, service, phase)
);

CREATE INDEX IF NOT EXISTS review_cases_pending_idx
    ON review_cases (created_at)
    WHERE status = 'pending';
