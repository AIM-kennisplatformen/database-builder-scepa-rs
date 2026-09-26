//! HTTP client for invoking SCEPA workflows through Restate ingress.

use reqwest::{Client, Url};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::models::draft::ManualDocument;

use super::workflows::{
    FixDocumentWorkflowRequest, FixDocumentWorkflowResponse, NewDocumentWorkflowRequest,
    NewDocumentWorkflowResponse, UpdateDocumentWorkflowRequest, UpdateDocumentWorkflowResponse,
};

/// Client for invoking all durable SCEPA workflows through Restate ingress.
#[derive(Clone)]
pub struct RestateClient {
    client: Client,
    ingress_url: Url,
}

/// A submitted new-document workflow that can be attached to later.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewDocumentWorkflowHandle {
    invocation_id: String,
    workflow_id: String,
}

/// A failure while invoking a workflow through Restate ingress.
#[derive(Debug, thiserror::Error)]
#[error("Restate workflow {workflow_id} failed: {detail}")]
pub struct RestateError {
    workflow_id: String,
    kind: RestateErrorKind,
    detail: String,
}

/// Stable failure categories exposed to HTTP adapters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestateErrorKind {
    InvalidRequest,
    Rejected(u16),
    Unavailable,
    Timeout,
    InvalidResponse,
}

impl RestateError {
    pub fn workflow_id(&self) -> &str {
        &self.workflow_id
    }

    pub fn kind(&self) -> RestateErrorKind {
        self.kind
    }

    pub fn rejection_message(&self) -> Option<&str> {
        matches!(self.kind, RestateErrorKind::Rejected(_)).then_some(self.detail.as_str())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendResponse {
    invocation_id: String,
}

impl RestateClient {
    /// Creates a client for a Restate ingress URL such as `http://localhost:8080`.
    pub fn new(ingress_url: &str) -> std::io::Result<Self> {
        Ok(Self {
            client: Client::new(),
            ingress_url: Url::parse(ingress_url).map_err(invalid_input)?,
        })
    }

    pub async fn run_new_document(
        &self,
        workflow_id: &str,
        pdf_hash: String,
    ) -> Result<NewDocumentWorkflowResponse, RestateError> {
        self.call_workflow(
            "NewDocumentWorkflow",
            workflow_id,
            &NewDocumentWorkflowRequest { pdf_hash },
        )
        .await
    }

    pub async fn run_update_document(
        &self,
        workflow_id: &str,
        request: UpdateDocumentWorkflowRequest,
    ) -> Result<UpdateDocumentWorkflowResponse, RestateError> {
        self.call_workflow("UpdateDocumentWorkflow", workflow_id, &request)
            .await
    }

    pub async fn publish_draft(
        &self,
        pdf_hash: String,
        manual_data: ManualDocument,
    ) -> Result<UpdateDocumentWorkflowResponse, RestateError> {
        self.update_with_key("draft", pdf_hash, manual_data).await
    }

    pub async fn update_document(
        &self,
        pdf_hash: String,
        manual_data: ManualDocument,
    ) -> Result<UpdateDocumentWorkflowResponse, RestateError> {
        self.update_with_key("update", pdf_hash, manual_data).await
    }

    pub async fn run_fix_document(
        &self,
        workflow_id: &str,
        request: FixDocumentWorkflowRequest,
    ) -> Result<FixDocumentWorkflowResponse, RestateError> {
        self.call_workflow("FixDocumentWorkflow", workflow_id, &request)
            .await
    }

    pub async fn fix_document(
        &self,
        request: FixDocumentWorkflowRequest,
    ) -> Result<FixDocumentWorkflowResponse, RestateError> {
        let invocation = uuid::Uuid::new_v4();
        self.run_fix_document(
            &format!("case:{}:fix:{invocation}", request.case_id),
            request,
        )
        .await
    }

    /// Starts a new-document workflow without waiting for its result.
    pub async fn submit_new_document(
        &self,
        workflow_id: &str,
        pdf_hash: String,
    ) -> Result<NewDocumentWorkflowHandle, RestateError> {
        let url = self
            .submit_url("NewDocumentWorkflow", workflow_id)
            .map_err(|error| invalid_request(workflow_id, error))?;
        let response = self
            .client
            .post(url)
            .json(&NewDocumentWorkflowRequest { pdf_hash })
            .send()
            .await
            .map_err(|error| connection_error(workflow_id, error))?;

        if !response.status().is_success() {
            return Err(response_error("NewDocumentWorkflow", workflow_id, response).await);
        }

        let response: SendResponse = response
            .json()
            .await
            .map_err(|error| response_decode_error(workflow_id, error))?;
        Ok(NewDocumentWorkflowHandle {
            invocation_id: response.invocation_id,
            workflow_id: workflow_id.to_owned(),
        })
    }

    /// Waits for the result of a previously submitted new-document workflow.
    pub async fn attach_new_document(
        &self,
        handle: &NewDocumentWorkflowHandle,
    ) -> Result<NewDocumentWorkflowResponse, RestateError> {
        let response = self
            .client
            .get(
                self.attach_url(&handle.invocation_id)
                    .map_err(|error| invalid_request(&handle.workflow_id, error))?,
            )
            .send()
            .await
            .map_err(|error| connection_error(&handle.workflow_id, error))?;

        if !response.status().is_success() {
            return Err(response_error("NewDocumentWorkflow", &handle.workflow_id, response).await);
        }

        response
            .json()
            .await
            .map_err(|error| response_decode_error(&handle.workflow_id, error))
    }

    async fn call_workflow<I, O>(
        &self,
        workflow: &str,
        workflow_id: &str,
        request: &I,
    ) -> Result<O, RestateError>
    where
        I: Serialize + ?Sized,
        O: DeserializeOwned,
    {
        let response = self
            .client
            .post(
                self.workflow_url(workflow, workflow_id)
                    .map_err(|error| invalid_request(workflow_id, error))?,
            )
            .json(request)
            .send()
            .await
            .map_err(|error| connection_error(workflow_id, error))?;

        if !response.status().is_success() {
            return Err(response_error(workflow, workflow_id, response).await);
        }

        response
            .json()
            .await
            .map_err(|error| response_decode_error(workflow_id, error))
    }

    async fn update_with_key(
        &self,
        operation: &str,
        pdf_hash: String,
        manual_data: ManualDocument,
    ) -> Result<UpdateDocumentWorkflowResponse, RestateError> {
        let workflow_id = format!("{pdf_hash}:{operation}:{}", uuid::Uuid::new_v4());
        self.run_update_document(
            &workflow_id,
            UpdateDocumentWorkflowRequest {
                pdf_hash,
                manual_data,
                review_case: None,
            },
        )
        .await
    }

    fn workflow_url(&self, workflow: &str, workflow_id: &str) -> std::io::Result<Url> {
        self.workflow_route(workflow, workflow_id, &["restate", "call"], &["run"])
    }

    fn submit_url(&self, workflow: &str, workflow_id: &str) -> std::io::Result<Url> {
        self.workflow_route(workflow, workflow_id, &[], &["run", "send"])
    }

    fn workflow_route(
        &self,
        workflow: &str,
        workflow_id: &str,
        prefix: &[&str],
        suffix: &[&str],
    ) -> std::io::Result<Url> {
        if workflow_id.is_empty() {
            return Err(invalid_input("workflow identifier must not be empty"));
        }

        let mut url = self.ingress_url.clone();
        let mut segments = url
            .path_segments_mut()
            .map_err(|()| invalid_input("Restate ingress URL cannot be a base URL"))?;
        segments.pop_if_empty();
        for segment in prefix {
            segments.push(segment);
        }
        segments.push(workflow).push(workflow_id);
        for segment in suffix {
            segments.push(segment);
        }
        drop(segments);
        Ok(url)
    }

    fn attach_url(&self, invocation_id: &str) -> std::io::Result<Url> {
        if invocation_id.is_empty() {
            return Err(invalid_input(
                "Restate invocation identifier must not be empty",
            ));
        }

        let mut url = self.ingress_url.clone();
        url.path_segments_mut()
            .map_err(|()| invalid_input("Restate ingress URL cannot be a base URL"))?
            .pop_if_empty()
            .push("restate")
            .push("invocation")
            .push(invocation_id)
            .push("attach");
        Ok(url)
    }
}

async fn response_error(
    workflow: &str,
    workflow_id: &str,
    response: reqwest::Response,
) -> RestateError {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    workflow_response_error(workflow, workflow_id, status, &body)
}

fn workflow_response_error(
    workflow: &str,
    workflow_id: &str,
    status: reqwest::StatusCode,
    body: &str,
) -> RestateError {
    let payload = serde_json::from_str::<serde_json::Value>(body).ok();
    if status == reqwest::StatusCode::CONFLICT
        || payload
            .as_ref()
            .and_then(|value| value.get("code"))
            .and_then(|code| code.as_u64())
            == Some(409)
    {
        let conflict = payload
            .as_ref()
            .and_then(|value| value.get("message"))
            .and_then(|message| message.as_str())
            .and_then(crate::conflict::Conflict::from_message)
            .unwrap_or(crate::conflict::Conflict::Submission);
        tracing::warn!(%workflow, %workflow_id, %status, "Restate workflow conflict");
        return RestateError {
            workflow_id: workflow_id.to_owned(),
            kind: RestateErrorKind::Rejected(409),
            detail: conflict.to_string(),
        };
    }
    let code = payload
        .as_ref()
        .and_then(|value| value.get("code"))
        .and_then(|code| code.as_u64())
        .and_then(|code| u16::try_from(code).ok())
        .filter(|code| (400..=599).contains(code))
        .unwrap_or(status.as_u16());
    let message = payload
        .as_ref()
        .and_then(|value| value.get("message"))
        .and_then(|message| message.as_str())
        .unwrap_or("Restate returned an unrecognized error response");
    tracing::error!(%workflow, %workflow_id, %status, response_body = %body, "Restate workflow failed");
    RestateError {
        workflow_id: workflow_id.to_owned(),
        kind: RestateErrorKind::Rejected(code),
        detail: message.to_owned(),
    }
}

fn invalid_input(error: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, error.to_string())
}

fn invalid_request(workflow_id: &str, error: std::io::Error) -> RestateError {
    RestateError {
        workflow_id: workflow_id.to_owned(),
        kind: RestateErrorKind::InvalidRequest,
        detail: error.to_string(),
    }
}

fn connection_error(workflow_id: &str, error: reqwest::Error) -> RestateError {
    let kind = if error.is_timeout() {
        RestateErrorKind::Timeout
    } else {
        RestateErrorKind::Unavailable
    };
    RestateError {
        workflow_id: workflow_id.to_owned(),
        kind,
        detail: error.to_string(),
    }
}

fn response_decode_error(workflow_id: &str, error: reqwest::Error) -> RestateError {
    RestateError {
        workflow_id: workflow_id.to_owned(),
        kind: RestateErrorKind::InvalidResponse,
        detail: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ordinary_requests_get_fresh_ids_but_explicit_ids_are_preserved() {
        use axum::{
            Router,
            body::Bytes,
            extract::State,
            http::{StatusCode, Uri},
        };
        use std::sync::{Arc, Mutex};
        let captured = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .fallback(
                |State(captured): State<Arc<Mutex<Vec<(String, Vec<u8>)>>>>,
                 uri: Uri,
                 body: Bytes| async move {
                    captured
                        .lock()
                        .unwrap()
                        .push((uri.path().to_owned(), body.to_vec()));
                    StatusCode::BAD_GATEWAY
                },
            )
            .with_state(captured.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client =
            RestateClient::new(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        for _ in 0..2 {
            assert!(
                client
                    .publish_draft("pdf".into(), ManualDocument::default())
                    .await
                    .is_err()
            );
            assert!(
                client
                    .update_document("pdf".into(), ManualDocument::default())
                    .await
                    .is_err()
            );
            assert!(
                client
                    .fix_document(FixDocumentWorkflowRequest {
                        case_id: 1,
                        manual_data: ManualDocument::default(),
                        enrich: false
                    })
                    .await
                    .is_err()
            );
            assert!(
                client
                    .run_new_document("explicit", "pdf".into())
                    .await
                    .is_err()
            );
            assert!(
                client
                    .submit_new_document("explicit", "pdf".into())
                    .await
                    .is_err()
            );
        }
        let requests = captured.lock().unwrap();
        for i in 0..3 {
            assert_ne!(requests[i].0, requests[i + 5].0);
            assert_eq!(requests[i].1, requests[i + 5].1);
        }
        assert_eq!(requests[3], requests[8]);
        assert_eq!(requests[4], requests[9]);
        server.abort();
    }

    #[test]
    fn conflict_responses_preserve_only_safe_messages() {
        for conflict in [
            crate::conflict::Conflict::WorkflowPdf,
            crate::conflict::Conflict::Record,
            crate::conflict::Conflict::CanonicalIdentity,
            crate::conflict::Conflict::PassageIdentity,
        ] {
            let body = serde_json::json!({"code":409,"message":conflict.to_string()}).to_string();
            let error =
                workflow_response_error("workflow", "key", reqwest::StatusCode::CONFLICT, &body);
            assert_eq!(error.kind(), RestateErrorKind::Rejected(409));
            assert_eq!(
                error.rejection_message(),
                Some(conflict.to_string().as_str())
            );
        }
        let error = workflow_response_error(
            "workflow",
            "key",
            reqwest::StatusCode::CONFLICT,
            "secret internal details",
        );
        assert_eq!(error.kind(), RestateErrorKind::Rejected(409));
        assert_eq!(
            error.rejection_message(),
            Some("Workflow has already been submitted")
        );
        assert_eq!(
            workflow_response_error(
                "workflow",
                "key",
                reqwest::StatusCode::BAD_GATEWAY,
                "offline"
            )
            .kind(),
            RestateErrorKind::Rejected(502)
        );
    }

    #[test]
    fn terminal_status_and_message_are_retained_for_safe_classification() {
        let error = workflow_response_error(
            "workflow",
            "upload:1",
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            r#"{"code":422,"message":"The extracted document could not be parsed"}"#,
        );
        assert_eq!(error.workflow_id(), "upload:1");
        assert_eq!(error.kind(), RestateErrorKind::Rejected(422));
        assert_eq!(
            error.rejection_message(),
            Some("The extracted document could not be parsed")
        );
    }

    #[test]
    fn workflow_keys_are_encoded_as_one_path_segment() {
        let client = RestateClient::new("http://localhost:8080").unwrap();
        assert_eq!(
            client
                .workflow_url("NewDocumentWorkflow", "folder/paper 1")
                .unwrap()
                .as_str(),
            "http://localhost:8080/restate/call/NewDocumentWorkflow/folder%2Fpaper%201/run"
        );
    }

    #[test]
    fn submission_and_attachment_urls_follow_restate_ingress_routes() {
        let client = RestateClient::new("http://localhost:8080").unwrap();
        assert_eq!(
            client
                .submit_url("NewDocumentWorkflow", "folder/paper 1")
                .unwrap()
                .as_str(),
            "http://localhost:8080/NewDocumentWorkflow/folder%2Fpaper%201/run/send"
        );
        assert_eq!(
            client.attach_url("invocation/id 1").unwrap().as_str(),
            "http://localhost:8080/restate/invocation/invocation%2Fid%201/attach"
        );
    }
}
