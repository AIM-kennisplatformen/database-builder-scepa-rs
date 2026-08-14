//! Garage-backed PDF ingestion pipeline and its persistence adapters.

use async_trait::async_trait;
use chrono::Utc;
use hmac::{Hmac, Mac};
use reqwest::{StatusCode, header};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;

use super::{
    FailureDisposition, PipelinePhase, PipelineService, ReviewArtifact, ReviewStore,
    ValidationReport,
};
use crate::postgres::PostgresReviewStore;

type HmacSha256 = Hmac<Sha256>;

/// PostgreSQL metadata for one immutable PDF object.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize, sqlx::FromRow)]
pub struct StoredPdf {
    pub pdf_hash: String,
    pub bucket: String,
    pub object_key: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub created_at: String,
}

#[derive(Clone)]
pub struct PostgresPdfStore {
    pool: PgPool,
}

impl PostgresPdfStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn upsert(&self, pdf: &StoredPdf) -> eros::Result<StoredPdf> {
        sqlx::query(
            r#"
            INSERT INTO document_artifacts (pdf_hash) VALUES ($1)
            ON CONFLICT (pdf_hash) DO NOTHING
            "#,
        )
        .bind(&pdf.pdf_hash)
        .execute(&self.pool)
        .await?;

        Ok(sqlx::query_as(
            r#"
            INSERT INTO pdf_files (pdf_hash, bucket, object_key, content_type, size_bytes)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (pdf_hash) DO UPDATE SET
                bucket = EXCLUDED.bucket,
                object_key = EXCLUDED.object_key,
                content_type = EXCLUDED.content_type,
                size_bytes = EXCLUDED.size_bytes
            RETURNING pdf_hash, bucket, object_key, content_type, size_bytes,
                      created_at::text AS created_at
            "#,
        )
        .bind(&pdf.pdf_hash)
        .bind(&pdf.bucket)
        .bind(&pdf.object_key)
        .bind(&pdf.content_type)
        .bind(pdf.size_bytes)
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn get(&self, pdf_hash: &str) -> eros::Result<Option<StoredPdf>> {
        Ok(sqlx::query_as(
            r#"
            SELECT pdf_hash, bucket, object_key, content_type, size_bytes,
                   created_at::text AS created_at
            FROM pdf_files WHERE pdf_hash = $1
            "#,
        )
        .bind(pdf_hash)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn link_workflow(&self, workflow_id: &str, pdf_hash: &str) -> eros::Result<()> {
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

    pub async fn hash_for_workflow(&self, workflow_id: &str) -> eros::Result<Option<String>> {
        Ok(
            sqlx::query_scalar("SELECT pdf_hash FROM workflow_pdfs WHERE workflow_id = $1")
                .bind(workflow_id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub async fn unlink_workflow(&self, workflow_id: &str) -> eros::Result<bool> {
        let result = sqlx::query("DELETE FROM workflow_pdfs WHERE workflow_id = $1")
            .bind(workflow_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() == 1)
    }
}

/// Minimal path-style S3 client for Garage using AWS Signature Version 4.
#[derive(Clone)]
pub struct GarageClient {
    client: reqwest::Client,
    endpoint: reqwest::Url,
    region: String,
    access_key: String,
    secret_key: String,
}

impl GarageClient {
    pub fn new(
        client: reqwest::Client,
        endpoint: &str,
        region: impl Into<String>,
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
    ) -> eros::Result<Self> {
        Ok(Self {
            client,
            endpoint: reqwest::Url::parse(endpoint)?,
            region: region.into(),
            access_key: access_key.into(),
            secret_key: secret_key.into(),
        })
    }

    pub async fn put(&self, bucket: &str, key: &str, bytes: &[u8]) -> eros::Result<()> {
        let url = self.object_url(bucket, key)?;
        let payload_hash = sha256_hex(bytes);
        let headers = self.signed_headers("PUT", &url, &payload_hash);
        let response = self
            .client
            .put(url)
            .headers(headers)
            .header(header::CONTENT_TYPE, "application/pdf")
            .body(bytes.to_vec())
            .send()
            .await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(
                std::io::Error::other(format!("Garage PUT failed with {status}: {body}")).into(),
            );
        }
        Ok(())
    }

    pub async fn get(&self, bucket: &str, key: &str) -> eros::Result<Option<Vec<u8>>> {
        let url = self.object_url(bucket, key)?;
        let payload_hash = sha256_hex(&[]);
        let headers = self.signed_headers("GET", &url, &payload_hash);
        let response = self.client.get(url).headers(headers).send().await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(
                std::io::Error::other(format!("Garage GET failed with {status}: {body}")).into(),
            );
        }
        Ok(Some(response.bytes().await?.to_vec()))
    }

    fn object_url(&self, bucket: &str, key: &str) -> eros::Result<reqwest::Url> {
        let mut url = self.endpoint.clone();
        let mut segments = url
            .path_segments_mut()
            .map_err(|()| std::io::Error::other("Garage endpoint cannot be a base URL"))?;
        segments.pop_if_empty().push(bucket).push(key);
        drop(segments);
        Ok(url)
    }

    fn signed_headers(
        &self,
        method: &str,
        url: &reqwest::Url,
        payload_hash: &str,
    ) -> header::HeaderMap {
        let now = Utc::now();
        let date = now.format("%Y%m%d").to_string();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let host = match url.port() {
            Some(port) => format!("{}:{port}", url.host_str().expect("object URL has host")),
            None => url.host_str().expect("object URL has host").to_owned(),
        };
        let signed = "host;x-amz-content-sha256;x-amz-date";
        let canonical_headers =
            format!("host:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n");
        let canonical_request = format!(
            "{method}\n{}\n\n{canonical_headers}\n{signed}\n{payload_hash}",
            url.path()
        );
        let scope = format!("{date}/{}/s3/aws4_request", self.region);
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
            sha256_hex(canonical_request.as_bytes())
        );
        let date_key = hmac(
            format!("AWS4{}", self.secret_key).as_bytes(),
            date.as_bytes(),
        );
        let region_key = hmac(&date_key, self.region.as_bytes());
        let service_key = hmac(&region_key, b"s3");
        let signing_key = hmac(&service_key, b"aws4_request");
        let signature = hex::encode(hmac(&signing_key, string_to_sign.as_bytes()));
        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed}, Signature={signature}",
            self.access_key
        );

        let mut headers = header::HeaderMap::new();
        headers.insert(header::HOST, host.parse().expect("valid host header"));
        headers.insert(
            "x-amz-content-sha256",
            payload_hash.parse().expect("valid payload hash header"),
        );
        headers.insert("x-amz-date", amz_date.parse().expect("valid date header"));
        headers.insert(
            header::AUTHORIZATION,
            authorization.parse().expect("valid authorization header"),
        );
        headers
    }
}

/// First pipeline service: validates and content-addresses a PDF, uploads it to
/// Garage, and records its immutable object location in PostgreSQL.
#[derive(Clone)]
pub struct GaragePipelineService {
    metadata: PostgresPdfStore,
    garage: GarageClient,
    bucket: String,
    review_store: PostgresReviewStore,
}

impl GaragePipelineService {
    pub fn new(
        metadata: PostgresPdfStore,
        garage: GarageClient,
        bucket: impl Into<String>,
        review_store: PostgresReviewStore,
    ) -> Self {
        Self {
            metadata,
            garage,
            bucket: bucket.into(),
            review_store,
        }
    }

    pub fn metadata(&self) -> &PostgresPdfStore {
        &self.metadata
    }

    async fn store(&self, bytes: &[u8]) -> eros::Result<StoredPdf> {
        let pdf_hash = sha256_hex(bytes);
        let object_key = format!("{pdf_hash}.pdf");
        self.garage.put(&self.bucket, &object_key, bytes).await?;
        self.metadata
            .upsert(&StoredPdf {
                pdf_hash,
                bucket: self.bucket.clone(),
                object_key,
                content_type: "application/pdf".into(),
                size_bytes: i64::try_from(bytes.len())?,
                created_at: String::new(),
            })
            .await
    }

    pub async fn load(&self, pdf_hash: &str) -> eros::Result<Option<(StoredPdf, Vec<u8>)>> {
        let Some(metadata) = self.metadata.get(pdf_hash).await? else {
            return Ok(None);
        };
        let Some(bytes) = self
            .garage
            .get(&metadata.bucket, &metadata.object_key)
            .await?
        else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("PDF {pdf_hash} is indexed in PostgreSQL but missing from Garage"),
            )
            .into());
        };
        Ok(Some((metadata, bytes)))
    }
}

#[async_trait]
impl PipelineService for GaragePipelineService {
    type Input = Vec<u8>;
    type Output = StoredPdf;
    type Warning = std::convert::Infallible;

    const NAME: &'static str = "garage";

    fn review_store(&self) -> &dyn ReviewStore {
        &self.review_store
    }

    fn failure_disposition(
        &self,
        phase: PipelinePhase,
        _error: &eros::ErrorUnion,
    ) -> FailureDisposition {
        match phase {
            PipelinePhase::Processing => FailureDisposition::Retryable,
            PipelinePhase::InputValidation | PipelinePhase::OutputValidation => {
                FailureDisposition::Terminal
            }
        }
    }

    async fn persist_input(&self, workflow_id: &str, bytes: &Self::Input) -> eros::Result<()> {
        self.review_store
            .store_pdf_hash(workflow_id, &sha256_hex(bytes))
            .await
    }

    async fn validate_input(
        &self,
        bytes: &Self::Input,
    ) -> eros::Result<ValidationReport<Self::Warning>> {
        if bytes.is_empty() {
            eros::bail!("PDF must not be empty")
        }
        if !bytes.starts_with(b"%PDF-") {
            eros::bail!("source bytes are not a PDF file")
        }
        Ok(ValidationReport::clean())
    }

    async fn process(&self, bytes: &Self::Input) -> eros::Result<Self::Output> {
        self.store(bytes).await
    }

    async fn validate_output(
        &self,
        stored: &Self::Output,
    ) -> eros::Result<ValidationReport<Self::Warning>> {
        if stored.pdf_hash.len() != 64
            || stored.object_key != format!("{}.pdf", stored.pdf_hash)
            || stored.size_bytes <= 0
        {
            eros::bail!("Garage returned invalid PDF metadata")
        }
        Ok(ValidationReport::clean())
    }

    fn review_artifact(
        &self,
        bytes: &Self::Input,
        _output: Option<&Self::Output>,
    ) -> ReviewArtifact {
        ReviewArtifact {
            content_type: "application/pdf".into(),
            bytes: bytes.clone(),
        }
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn hmac(key: &[u8], message: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts arbitrary key lengths");
    mac.update(message);
    mac.finalize().into_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::postgres::PostgresReviewStore;

    #[test]
    fn sha256_is_lowercase_content_digest() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[tokio::test]
    #[ignore = "requires local Garage and PostgreSQL services"]
    async fn garage_round_trip_is_resolved_through_postgres() {
        let review_store =
            PostgresReviewStore::connect_lazy("postgres://scepa:scepa@localhost:5432/scepa")
                .unwrap();
        review_store.migrate().await.unwrap();
        let client = reqwest::Client::new();
        let garage = GarageClient::new(
            client,
            "http://localhost:3900",
            "garage",
            "GK00000000000000000000000000000000",
            "0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap();
        let service = GaragePipelineService::new(
            PostgresPdfStore::new(review_store.pool().clone()),
            garage,
            "scepa-pdfs",
            review_store,
        );
        let bytes = b"%PDF-1.4\n% SCEPA Garage integration fixture\n";

        let stored = service.process(&bytes.to_vec()).await.unwrap();
        let (indexed, loaded) = service.load(&stored.pdf_hash).await.unwrap().unwrap();

        assert_eq!(indexed.object_key, format!("{}.pdf", stored.pdf_hash));
        assert_eq!(loaded, bytes);
    }
}
