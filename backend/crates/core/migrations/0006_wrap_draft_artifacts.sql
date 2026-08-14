UPDATE document_artifacts
SET draft_artifact = jsonb_build_object(
        'pdf_hash', pdf_hash,
        'grobid_extraction_data', draft_artifact,
        'manual_data', jsonb_build_object('bibliography', jsonb_build_object())
    ),
    updated_at = now()
WHERE draft_artifact IS NOT NULL
  AND NOT (draft_artifact ? 'grobid_extraction_data');
