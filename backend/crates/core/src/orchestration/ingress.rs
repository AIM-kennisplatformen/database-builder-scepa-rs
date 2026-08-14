//! HTTP client for invoking SCEPA workflows through Restate ingress.

use reqwest::{Client, Url};
use serde::Deserialize;

use super::{
    NewDocumentWorkflowRequest, NewDocumentWorkflowResponse, UpdateDocumentWorkflowRequest,
    UpdateDocumentWorkflowResponse,
};

/// Client for submitting documents to the durable new-document workflow.
#[derive(Clone)]
pub struct NewDocumentIngressClient {
    client: Client,
    ingress_url: Url,
}

/// Client for invoking durable updates of already-published documents.
#[derive(Clone)]
pub struct UpdateDocumentIngressClient {
    client: Client,
    ingress_url: Url,
}

/// A submitted workflow that can be attached to later.
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

impl NewDocumentIngressClient {
    /// Creates a client for a Restate ingress URL such as `http://localhost:8080`.
    pub fn new(ingress_url: &str) -> std::io::Result<Self> {
        Ok(Self {
            client: Client::new(),
            ingress_url: Url::parse(ingress_url).map_err(invalid_input)?,
        })
    }

    /// Invokes one workflow and waits for its durable result.
    pub async fn run(
        &self,
        workflow_id: &str,
        pdf_hash: String,
    ) -> std::io::Result<NewDocumentWorkflowResponse> {
        let url = self.workflow_url(workflow_id)?;
        let response = self
            .client
            .post(url)
            .json(&NewDocumentWorkflowRequest { pdf_hash })
            .send()
            .await
            .map_err(connection_error)?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(std::io::Error::other(format!(
                "NewDocumentWorkflow {workflow_id} failed through Restate with {status}: {body}"
            )));
        }

        response.json().await.map_err(connection_error)
    }

    /// Starts a workflow without waiting for its result.
    pub async fn submit(
        &self,
        workflow_id: &str,
        pdf_hash: String,
    ) -> std::io::Result<NewDocumentWorkflowHandle> {
        let url = self.submit_url(workflow_id)?;
        let response = self
            .client
            .post(url)
            .json(&NewDocumentWorkflowRequest { pdf_hash })
            .send()
            .await
            .map_err(connection_error)?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(std::io::Error::other(format!(
                "could not submit NewDocumentWorkflow {workflow_id} through Restate with {status}: {body}"
            )));
        }

        let response: SendResponse = response.json().await.map_err(connection_error)?;
        Ok(NewDocumentWorkflowHandle {
            invocation_id: response.invocation_id,
            workflow_id: workflow_id.to_owned(),
        })
    }

    /// Waits for the result of a previously submitted workflow.
    pub async fn attach(
        &self,
        handle: &NewDocumentWorkflowHandle,
    ) -> std::io::Result<NewDocumentWorkflowResponse> {
        let url = self.attach_url(&handle.invocation_id)?;
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(connection_error)?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(std::io::Error::other(format!(
                "NewDocumentWorkflow {} failed through Restate with {status}: {body}",
                handle.workflow_id
            )));
        }

        response.json().await.map_err(connection_error)
    }

    fn workflow_url(&self, workflow_id: &str) -> std::io::Result<Url> {
        if workflow_id.is_empty() {
            return Err(invalid_input("workflow identifier must not be empty"));
        }

        let mut url = self.ingress_url.clone();
        let mut segments = url.path_segments_mut().map_err(|()| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Restate ingress URL cannot be a base URL",
            )
        })?;
        segments
            .pop_if_empty()
            .push("restate")
            .push("call")
            .push("NewDocumentWorkflow")
            .push(workflow_id)
            .push("run");
        drop(segments);
        Ok(url)
    }

    fn submit_url(&self, workflow_id: &str) -> std::io::Result<Url> {
        if workflow_id.is_empty() {
            return Err(invalid_input("workflow identifier must not be empty"));
        }

        let mut url = self.ingress_url.clone();
        let mut segments = url.path_segments_mut().map_err(|()| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Restate ingress URL cannot be a base URL",
            )
        })?;
        segments
            .pop_if_empty()
            .push("NewDocumentWorkflow")
            .push(workflow_id)
            .push("run")
            .push("send");
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
        let mut segments = url.path_segments_mut().map_err(|()| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Restate ingress URL cannot be a base URL",
            )
        })?;
        segments
            .pop_if_empty()
            .push("restate")
            .push("invocation")
            .push(invocation_id)
            .push("attach");
        drop(segments);
        Ok(url)
    }
}

impl UpdateDocumentIngressClient {
    pub fn new(ingress_url: &str) -> std::io::Result<Self> {
        Ok(Self {
            client: Client::new(),
            ingress_url: Url::parse(ingress_url).map_err(invalid_input)?,
        })
    }

    pub async fn run(
        &self,
        workflow_id: &str,
        request: UpdateDocumentWorkflowRequest,
    ) -> std::io::Result<UpdateDocumentWorkflowResponse> {
        if workflow_id.is_empty() {
            return Err(invalid_input("workflow identifier must not be empty"));
        }
        let mut url = self.ingress_url.clone();
        let mut segments = url
            .path_segments_mut()
            .map_err(|()| invalid_input("Restate ingress URL cannot be a base URL"))?;
        segments
            .pop_if_empty()
            .push("restate")
            .push("call")
            .push("UpdateDocumentWorkflow")
            .push(workflow_id)
            .push("run");
        drop(segments);

        let response = self
            .client
            .post(url)
            .json(&request)
            .send()
            .await
            .map_err(connection_error)?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(std::io::Error::other(format!(
                "UpdateDocumentWorkflow {workflow_id} failed through Restate with {status}: {body}"
            )));
        }
        response.json().await.map_err(connection_error)
    }
}

fn invalid_input(error: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, error.to_string())
}

fn connection_error(error: reqwest::Error) -> std::io::Error {
    std::io::Error::other(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_keys_are_encoded_as_one_path_segment() {
        let client = NewDocumentIngressClient::new("http://localhost:8080").unwrap();
        assert_eq!(
            client.workflow_url("folder/paper 1").unwrap().as_str(),
            "http://localhost:8080/restate/call/NewDocumentWorkflow/folder%2Fpaper%201/run"
        );
    }

    #[test]
    fn submission_and_attachment_urls_follow_restate_ingress_routes() {
        let client = NewDocumentIngressClient::new("http://localhost:8080").unwrap();
        assert_eq!(
            client.submit_url("folder/paper 1").unwrap().as_str(),
            "http://localhost:8080/NewDocumentWorkflow/folder%2Fpaper%201/run/send"
        );
        assert_eq!(
            client.attach_url("invocation/id 1").unwrap().as_str(),
            "http://localhost:8080/restate/invocation/invocation%2Fid%201/attach"
        );
    }
}
