//! PostgreSQL persistence for pipeline failures requiring operator review.

use async_trait::async_trait;
use serde::Serialize;
use sqlx::{PgPool, postgres::PgPoolOptions};

use crate::pipeline::{FailureDisposition, FailureRecord, ReviewStore};

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
