CREATE TABLE IF NOT EXISTS document_artifacts (
    pdf_hash text PRIMARY KEY CHECK (pdf_hash ~ '^[0-9a-f]{64}$'),
    tei_xml text,
    draft_artifact jsonb,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

INSERT INTO document_artifacts (pdf_hash)
SELECT pdf_hash FROM pdf_files
ON CONFLICT (pdf_hash) DO NOTHING;

ALTER TABLE workflow_pdfs
    DROP CONSTRAINT IF EXISTS workflow_pdfs_pdf_hash_fkey;

ALTER TABLE workflow_pdfs
    ADD CONSTRAINT workflow_pdfs_pdf_hash_fkey
    FOREIGN KEY (pdf_hash) REFERENCES document_artifacts(pdf_hash) ON DELETE CASCADE;

ALTER TABLE review_cases
    DROP CONSTRAINT IF EXISTS review_cases_pdf_hash_fkey;

ALTER TABLE review_cases
    ADD CONSTRAINT review_cases_pdf_hash_fkey
    FOREIGN KEY (pdf_hash) REFERENCES document_artifacts(pdf_hash) ON DELETE SET NULL;

ALTER TABLE pdf_files
    DROP CONSTRAINT IF EXISTS pdf_files_document_artifact_fkey;

ALTER TABLE pdf_files
    ADD CONSTRAINT pdf_files_document_artifact_fkey
    FOREIGN KEY (pdf_hash) REFERENCES document_artifacts(pdf_hash) ON DELETE CASCADE;

UPDATE document_artifacts AS document
SET tei_xml = convert_from(review.artifact_bytes, 'UTF8'),
    updated_at = now()
FROM review_cases AS review
WHERE review.pdf_hash = document.pdf_hash
  AND split_part(review.artifact_content_type, ';', 1) = 'application/tei+xml'
  AND document.tei_xml IS NULL;

WITH review_drafts AS (
    SELECT pdf_hash,
           CASE
               WHEN pg_input_is_valid(convert_from(artifact_bytes, 'UTF8'), 'jsonb')
               THEN convert_from(artifact_bytes, 'UTF8')::jsonb
           END AS draft_artifact
    FROM review_cases
    WHERE split_part(artifact_content_type, ';', 1) = 'application/json'
      AND octet_length(artifact_bytes) > 0
)
UPDATE document_artifacts AS document
SET draft_artifact = review.draft_artifact,
    updated_at = now()
FROM review_drafts AS review
WHERE review.pdf_hash = document.pdf_hash
  AND review.draft_artifact IS NOT NULL
  AND document.draft_artifact IS NULL;
