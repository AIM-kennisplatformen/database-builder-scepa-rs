//! HTTP client for invoking SCEPA workflows through Restate ingress.

use reqwest::{Client, Url};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{models::draft::ManualDocument, pipeline::garage::sha256_hex};

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
    ) -> std::io::Result<NewDocumentWorkflowResponse> {
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
    ) -> std::io::Result<UpdateDocumentWorkflowResponse> {
        self.call_workflow("UpdateDocumentWorkflow", workflow_id, &request)
            .await
    }

    pub async fn publish_draft(
        &self,
        pdf_hash: String,
        manual_data: ManualDocument,
    ) -> std::io::Result<UpdateDocumentWorkflowResponse> {
        self.update_with_key("draft", pdf_hash, manual_data).await
    }

    pub async fn update_document(
        &self,
        pdf_hash: String,
        manual_data: ManualDocument,
    ) -> std::io::Result<UpdateDocumentWorkflowResponse> {
        self.update_with_key("update", pdf_hash, manual_data).await
    }

    pub async fn run_fix_document(
        &self,
        workflow_id: &str,
        request: FixDocumentWorkflowRequest,
    ) -> std::io::Result<FixDocumentWorkflowResponse> {
        self.call_workflow("FixDocumentWorkflow", workflow_id, &request)
            .await
    }

    pub async fn fix_document(
        &self,
        request: FixDocumentWorkflowRequest,
    ) -> std::io::Result<FixDocumentWorkflowResponse> {
        let revision = revision(&request.manual_data)?;
        self.run_fix_document(&format!("case:{}:fix:{revision}", request.case_id), request)
            .await
    }

    /// Starts a new-document workflow without waiting for its result.
    pub async fn submit_new_document(
        &self,
        workflow_id: &str,
        pdf_hash: String,
    ) -> std::io::Result<NewDocumentWorkflowHandle> {
        let url = self.submit_url("NewDocumentWorkflow", workflow_id)?;
        let response = self
            .client
            .post(url)
            .json(&NewDocumentWorkflowRequest { pdf_hash })
            .send()
            .await
            .map_err(connection_error)?;

        if !response.status().is_success() {
            return Err(response_error("NewDocumentWorkflow", workflow_id, response).await);
        }

        let response: SendResponse = response.json().await.map_err(connection_error)?;
        Ok(NewDocumentWorkflowHandle {
            invocation_id: response.invocation_id,
            workflow_id: workflow_id.to_owned(),
        })
    }

    /// Waits for the result of a previously submitted new-document workflow.
    pub async fn attach_new_document(
        &self,
        handle: &NewDocumentWorkflowHandle,
    ) -> std::io::Result<NewDocumentWorkflowResponse> {
        let response = self
            .client
            .get(self.attach_url(&handle.invocation_id)?)
            .send()
            .await
            .map_err(connection_error)?;

        if !response.status().is_success() {
            return Err(response_error("NewDocumentWorkflow", &handle.workflow_id, response).await);
        }

        response.json().await.map_err(connection_error)
    }

    async fn call_workflow<I, O>(
        &self,
        workflow: &str,
        workflow_id: &str,
        request: &I,
    ) -> std::io::Result<O>
    where
        I: Serialize + ?Sized,
        O: DeserializeOwned,
    {
        let response = self
            .client
            .post(self.workflow_url(workflow, workflow_id)?)
            .json(request)
            .send()
            .await
            .map_err(connection_error)?;

        if !response.status().is_success() {
            return Err(response_error(workflow, workflow_id, response).await);
        }

        response.json().await.map_err(connection_error)
    }

    async fn update_with_key(
        &self,
        operation: &str,
        pdf_hash: String,
        manual_data: ManualDocument,
    ) -> std::io::Result<UpdateDocumentWorkflowResponse> {
        let workflow_id = format!("{pdf_hash}:{operation}:{}", revision(&manual_data)?);
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
) -> std::io::Error {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    std::io::Error::other(format!(
        "{workflow} {workflow_id} failed through Restate with {status}: {body}"
    ))
}

fn invalid_input(error: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, error.to_string())
}

fn connection_error(error: reqwest::Error) -> std::io::Error {
    std::io::Error::other(error)
}

fn revision(manual_data: &ManualDocument) -> std::io::Result<String> {
    serde_json::to_vec(manual_data)
        .map(|bytes| sha256_hex(&bytes))
        .map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

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
