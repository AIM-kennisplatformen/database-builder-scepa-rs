CREATE TABLE IF NOT EXISTS pdf_files (
    pdf_hash text PRIMARY KEY CHECK (pdf_hash ~ '^[0-9a-f]{64}$'),
    bucket text NOT NULL,
    object_key text NOT NULL,
    content_type text NOT NULL DEFAULT 'application/pdf',
    size_bytes bigint NOT NULL CHECK (size_bytes > 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (bucket, object_key)
);

CREATE TABLE IF NOT EXISTS workflow_pdfs (
    workflow_id text PRIMARY KEY,
    pdf_hash text NOT NULL REFERENCES pdf_files(pdf_hash) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS workflow_pdfs_hash_idx ON workflow_pdfs (pdf_hash);
