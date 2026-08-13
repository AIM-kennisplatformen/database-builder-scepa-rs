ALTER TABLE review_cases
    ADD COLUMN IF NOT EXISTS pdf_hash text
    REFERENCES pdf_files(pdf_hash) ON DELETE SET NULL;

UPDATE review_cases AS review_case
SET pdf_hash = workflow_pdf.pdf_hash
FROM workflow_pdfs AS workflow_pdf
WHERE review_case.workflow_id = workflow_pdf.workflow_id
  AND review_case.pdf_hash IS NULL;

