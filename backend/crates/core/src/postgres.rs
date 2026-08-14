//! PostgreSQL persistence for pipeline failures requiring operator review.

use async_trait::async_trait;
use serde::Serialize;
use sqlx::{PgPool, postgres::PgPoolOptions};

use crate::models::draft::{DraftDocument, ManualDocument};
use crate::pipeline::{
    FailureDisposition, FailureRecord, ReviewStore, document::DocumentArtifactStore,
};

/// Data retained for every document as soon as each representation exists.
#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct DocumentArtifacts {
    pub pdf_hash: String,
    pub tei_xml: Option<String>,
    pub draft_artifact: Option<serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
}

/// A document that has successfully reached the canonical graph.
#[derive(Clone, Debug, Serialize)]
pub struct PublishedDocument {
    pub pdf_hash: String,
    pub artifact: DraftDocument,
    pub published_at: String,
}

/// Metadata used by the update-document picker.
#[derive(Clone, Debug, Serialize)]
pub struct PublishedDocumentSummary {
    pub pdf_hash: String,
    pub title: Option<String>,
    pub identifiers: Vec<crate::models::draft::Identifier>,
    pub published_at: String,
}

/// A durable, idempotent [`ReviewStore`] backed by PostgreSQL.
///
/// An orchestrator can replay a successful database write before journaling
/// its result. The database uniqueness constraint on workflow,
/// service, and phase makes that replay a no-op instead of creating a duplicate
/// review case.
#[derive(Clone)]
pub struct PostgresReviewStore {
    pool: PgPool,
}

/// Review-case metadata returned by the operator API.
#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct ReviewCase {
    pub id: i64,
    pub workflow_id: String,
    pub pdf_hash: Option<String>,
    pub service: String,
    pub phase: String,
    pub retryable: bool,
    pub error_message: String,
    pub artifact_content_type: String,
    pub artifact_size: i32,
    pub status: String,
    pub resolution: Option<serde_json::Value>,
    pub created_at: String,
    pub resolved_at: Option<String>,
}

impl PostgresReviewStore {
    /// Connects to PostgreSQL using a lazily established connection pool.
    pub fn connect_lazy(database_url: &str) -> eros::Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect_lazy(database_url)?;
        Ok(Self { pool })
    }

    /// Wraps an existing PostgreSQL pool.
    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Returns the underlying pool for application-specific queries.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Applies all embedded database migrations.
    pub async fn migrate(&self) -> eros::Result<()> {
        sqlx::migrate!().run(&self.pool).await?;
        Ok(())
    }

    /// Persists a content hash and its workflow association before processing.
    pub async fn store_pdf_hash(&self, workflow_id: &str, pdf_hash: &str) -> eros::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO document_artifacts (pdf_hash) VALUES ($1)
            ON CONFLICT (pdf_hash) DO NOTHING
            "#,
        )
        .bind(pdf_hash)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO workflow_pdfs (workflow_id, pdf_hash) VALUES ($1, $2)
            ON CONFLICT (workflow_id) DO NOTHING
            "#,
        )
        .bind(workflow_id)
        .bind(pdf_hash)
        .execute(&self.pool)
        .await?;

        let linked_hash: String =
            sqlx::query_scalar("SELECT pdf_hash FROM workflow_pdfs WHERE workflow_id = $1")
                .bind(workflow_id)
                .fetch_one(&self.pool)
                .await?;
        if linked_hash != pdf_hash {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("workflow {workflow_id} is already linked to PDF {linked_hash}"),
            )
            .into());
        }

        Ok(())
    }

    /// Persists raw TEI immediately after extraction.
    pub async fn store_tei_xml(&self, pdf_hash: &str, tei_xml: &str) -> eros::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO document_artifacts (pdf_hash, tei_xml)
            VALUES ($1, $2)
            ON CONFLICT (pdf_hash) DO UPDATE
            SET tei_xml = EXCLUDED.tei_xml, updated_at = now()
            "#,
        )
        .bind(pdf_hash)
        .bind(tei_xml)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Persists the converted draft immediately after conversion.
    pub async fn store_draft_artifact(
        &self,
        pdf_hash: &str,
        draft: &DraftDocument,
    ) -> eros::Result<()> {
        let mut artifact = serde_json::to_value(draft)?;
        artifact
            .as_object_mut()
            .expect("DraftDocument serializes as a JSON object")
            .insert("pdf_hash".into(), pdf_hash.into());
        sqlx::query(
            r#"
            INSERT INTO document_artifacts (pdf_hash, draft_artifact)
            VALUES ($1, $2)
            ON CONFLICT (pdf_hash) DO UPDATE
            SET draft_artifact = EXCLUDED.draft_artifact, updated_at = now()
            "#,
        )
        .bind(pdf_hash)
        .bind(artifact)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Loads the typed review draft without discarding its extraction data.
    pub async fn get_draft_document(&self, pdf_hash: &str) -> eros::Result<Option<DraftDocument>> {
        let artifact: Option<serde_json::Value> =
            sqlx::query_scalar("SELECT draft_artifact FROM document_artifacts WHERE pdf_hash = $1")
                .bind(pdf_hash)
                .fetch_optional(&self.pool)
                .await?
                .flatten();

        artifact
            .map(serde_json::from_value)
            .transpose()
            .map_err(Into::into)
    }

    /// Replaces only the operator-authored layer of an existing draft.
    pub async fn store_manual_data(
        &self,
        pdf_hash: &str,
        manual_data: &ManualDocument,
    ) -> eros::Result<bool> {
        let result = sqlx::query(
            r#"
            UPDATE document_artifacts
            SET draft_artifact = jsonb_set(
                    draft_artifact,
                    '{manual_data}',
                    $2::jsonb,
                    true
                ),
                updated_at = now()
            WHERE pdf_hash = $1 AND draft_artifact IS NOT NULL
            "#,
        )
        .bind(pdf_hash)
        .bind(serde_json::to_value(manual_data)?)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() == 1)
    }

    /// Records the exact artifact that was accepted by the canonical graph.
    pub async fn store_published_artifact(
        &self,
        pdf_hash: &str,
        artifact: &DraftDocument,
    ) -> eros::Result<bool> {
        let result = sqlx::query(
            r#"
            UPDATE document_artifacts
            SET draft_artifact = $2,
                published_artifact = $2,
                published_at = now(),
                updated_at = now()
            WHERE pdf_hash = $1
            "#,
        )
        .bind(pdf_hash)
        .bind(serde_json::to_value(artifact)?)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() == 1)
    }

    /// Loads the last artifact successfully accepted by TypeDB.
    pub async fn get_published_document(
        &self,
        pdf_hash: &str,
    ) -> eros::Result<Option<PublishedDocument>> {
        let row: Option<(serde_json::Value, String)> = sqlx::query_as(
            r#"
            SELECT published_artifact, published_at::text
            FROM document_artifacts
            WHERE pdf_hash = $1 AND published_artifact IS NOT NULL
            "#,
        )
        .bind(pdf_hash)
        .fetch_optional(&self.pool)
        .await?;

        row.map(|(artifact, published_at)| {
            Ok(PublishedDocument {
                pdf_hash: pdf_hash.to_owned(),
                artifact: serde_json::from_value(artifact)?,
                published_at,
            })
        })
        .transpose()
    }

    /// Lists only documents that completed canonical publication successfully.
    pub async fn list_published_documents(&self) -> eros::Result<Vec<PublishedDocumentSummary>> {
        let rows: Vec<(String, serde_json::Value, String)> = sqlx::query_as(
            r#"
            SELECT pdf_hash, published_artifact, published_at::text
            FROM document_artifacts
            WHERE published_artifact IS NOT NULL
            ORDER BY published_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|(pdf_hash, value, published_at)| {
                let artifact: DraftDocument = serde_json::from_value(value)?;
                let effective = artifact.effective_document();
                Ok(PublishedDocumentSummary {
                    pdf_hash,
                    title: effective.bibliography.title,
                    identifiers: effective.bibliography.identifiers,
                    published_at,
                })
            })
            .collect()
    }

    /// Loads all representations currently available for one document.
    pub async fn get_document_artifacts(
        &self,
        pdf_hash: &str,
    ) -> eros::Result<Option<DocumentArtifacts>> {
        Ok(sqlx::query_as(
            r#"
            SELECT pdf_hash, tei_xml, draft_artifact,
                   created_at::text AS created_at, updated_at::text AS updated_at
            FROM document_artifacts
            WHERE pdf_hash = $1
            "#,
        )
        .bind(pdf_hash)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// Lists review cases, optionally filtered by status.
    pub async fn list_cases(
        &self,
        status: Option<&str>,
        limit: i64,
    ) -> eros::Result<Vec<ReviewCase>> {
        Ok(sqlx::query_as::<_, ReviewCase>(
            r#"
            SELECT
                id,
                workflow_id,
                pdf_hash,
                service,
                phase,
                retryable,
                error_message,
                artifact_content_type,
                octet_length(artifact_bytes) AS artifact_size,
                status,
                resolution,
                created_at::text AS created_at,
                resolved_at::text AS resolved_at
            FROM review_cases
            WHERE $1::text IS NULL OR status = $1
            ORDER BY created_at DESC
            LIMIT $2
            "#,
        )
        .bind(status)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Counts review cases that are waiting for an operator decision.
    pub async fn count_pending_cases(&self) -> eros::Result<i64> {
        Ok(
            sqlx::query_scalar("SELECT count(*) FROM review_cases WHERE status = 'pending'")
                .fetch_one(&self.pool)
                .await?,
        )
    }

    /// Returns one review case without loading its artifact payload.
    pub async fn get_case(&self, id: i64) -> eros::Result<Option<ReviewCase>> {
        Ok(sqlx::query_as::<_, ReviewCase>(
            r#"
            SELECT
                id,
                workflow_id,
                pdf_hash,
                service,
                phase,
                retryable,
                error_message,
                artifact_content_type,
                octet_length(artifact_bytes) AS artifact_size,
                status,
                resolution,
                created_at::text AS created_at,
                resolved_at::text AS resolved_at
            FROM review_cases
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// Loads the content type and bytes for a review artifact.
    pub async fn get_artifact(&self, id: i64) -> eros::Result<Option<(String, Vec<u8>)>> {
        Ok(sqlx::query_as(
            r#"
            SELECT artifact_content_type, artifact_bytes
            FROM review_cases
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// Replaces the artifact for a pending validation failure.
    ///
    /// Processing artifacts are deliberately immutable: patching is intended
    /// for operator repair of input- or output-validation failures only.
    pub async fn patch_validation_artifact(
        &self,
        id: i64,
        content_type: &str,
        bytes: &[u8],
    ) -> eros::Result<bool> {
        let result = sqlx::query(
            r#"
            UPDATE review_cases
            SET artifact_content_type = $2, artifact_bytes = $3
            WHERE id = $1
              AND status = 'pending'
              AND phase IN ('input_validation', 'output_validation')
            "#,
        )
        .bind(id)
        .bind(content_type)
        .bind(bytes)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() == 1)
    }

    /// Deletes every review case and stored review artifact for a workflow.
    pub async fn delete_cases_for_workflow(&self, workflow_id: &str) -> eros::Result<u64> {
        let result = sqlx::query("DELETE FROM review_cases WHERE workflow_id = $1")
            .bind(workflow_id)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected())
    }

    /// Records an operator decision if the case belongs to the given workflow.
    pub async fn resolve_case(
        &self,
        id: i64,
        workflow_id: &str,
        status: &str,
        resolution: serde_json::Value,
    ) -> eros::Result<bool> {
        let result = sqlx::query(
            r#"
            UPDATE review_cases
            SET status = $3, resolution = $4, resolved_at = now()
            WHERE id = $1 AND workflow_id = $2 AND status = 'pending'
            "#,
        )
        .bind(id)
        .bind(workflow_id)
        .bind(status)
        .bind(resolution)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() == 1)
    }
}

#[async_trait]
impl ReviewStore for PostgresReviewStore {
    async fn stage(&self, failure: FailureRecord) -> eros::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO review_cases (
                workflow_id,
                pdf_hash,
                service,
                phase,
                retryable,
                error_message,
                artifact_content_type,
                artifact_bytes
            )
            VALUES (
                $1,
                (SELECT pdf_hash FROM workflow_pdfs WHERE workflow_id = $1),
                $2,
                $3,
                $4,
                $5,
                $6,
                $7
            )
            ON CONFLICT (workflow_id, service, phase) DO UPDATE
            SET pdf_hash = COALESCE(review_cases.pdf_hash, EXCLUDED.pdf_hash)
            "#,
        )
        .bind(failure.workflow_id)
        .bind(failure.service)
        .bind(failure.phase.as_str())
        .bind(failure.disposition == FailureDisposition::Retryable)
        .bind(failure.error_message)
        .bind(failure.artifact.content_type)
        .bind(failure.artifact.bytes)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

#[async_trait]
impl DocumentArtifactStore for PostgresReviewStore {
    async fn store_tei_xml(&self, pdf_hash: &str, tei_xml: &str) -> eros::Result<()> {
        PostgresReviewStore::store_tei_xml(self, pdf_hash, tei_xml).await
    }

    async fn store_draft_artifact(
        &self,
        pdf_hash: &str,
        draft: &DraftDocument,
    ) -> eros::Result<()> {
        PostgresReviewStore::store_draft_artifact(self, pdf_hash, draft).await
    }
}
