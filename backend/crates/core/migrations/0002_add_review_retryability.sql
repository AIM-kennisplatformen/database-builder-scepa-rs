ALTER TABLE review_cases
    ADD COLUMN IF NOT EXISTS retryable boolean NOT NULL DEFAULT false;
