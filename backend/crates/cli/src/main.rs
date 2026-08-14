use std::{
    env,
    error::Error,
    path::{Path, PathBuf},
};

use clap::{Parser, Subcommand};
use reqwest::{Client, Url, header};
use tokio::task::JoinSet;

#[derive(Debug, Parser)]
#[command(
    name = "scepa-cli",
    about = "SCEPA pipeline command-line interface",
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Uploads a single PDF.
    Single {
        /// Overrides the PDF file stem used as the workflow identifier.
        #[arg(long)]
        identifier: Option<String>,
        /// PDF to upload.
        pdf: PathBuf,
    },
    /// Uploads every PDF in a directory.
    Batch {
        /// Directory containing PDFs to upload.
        directory: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_url = env::var("SCEPA_API_URL").unwrap_or_else(|_| "http://localhost:3000".into());
    let uploads = UploadClient::new(&api_url)?;

    match Cli::parse().command {
        Command::Single { identifier, pdf } => upload_pdf(&uploads, pdf, identifier).await?,
        Command::Batch { directory } => upload_batch(&uploads, directory).await?,
    }

    Ok(())
}

async fn upload_pdf(
    uploads: &UploadClient,
    pdf: PathBuf,
    identifier: Option<String>,
) -> Result<(), Box<dyn Error>> {
    let workflow_id = workflow_identifier(&pdf, identifier)?;
    let bytes = tokio::fs::read(&pdf).await?;
    uploads.submit(&workflow_id, bytes).await?;

    Ok(())
}

fn workflow_identifier(pdf: &Path, identifier: Option<String>) -> Result<String, Box<dyn Error>> {
    let workflow_id = match identifier {
        Some(identifier) if !identifier.is_empty() => identifier,
        Some(_) => return Err("workflow identifier must not be empty".into()),
        None => pdf
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty())
            .ok_or("PDF path has no valid UTF-8 file stem")?
            .to_owned(),
    };
    Ok(workflow_id)
}

async fn upload_batch(uploads: &UploadClient, directory: PathBuf) -> Result<(), Box<dyn Error>> {
    let mut entries = tokio::fs::read_dir(&directory).await?;
    let mut submissions = JoinSet::new();

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if entry.file_type().await?.is_file()
            && path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
        {
            let uploads = uploads.clone();
            submissions.spawn(async move {
                upload_pdf(&uploads, path, None)
                    .await
                    .map_err(|error| error.to_string())
            });
        }
    }

    let mut first_error = None;
    while let Some(submission) = submissions.join_next().await {
        let result = submission
            .map_err(|error| error.to_string())
            .and_then(|result| result);
        if let Err(error) = result
            && first_error.is_none()
        {
            first_error = Some(error);
        }
    }

    if let Some(error) = first_error {
        return Err(error.into());
    }

    Ok(())
}

#[derive(Clone)]
struct UploadClient {
    client: Client,
    api_url: Url,
}

impl UploadClient {
    fn new(api_url: &str) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            client: Client::new(),
            api_url: Url::parse(api_url)?,
        })
    }

    async fn submit(&self, workflow_id: &str, pdf: Vec<u8>) -> Result<(), Box<dyn Error>> {
        let url = self.submission_url(workflow_id)?;

        let response = self
            .client
            .post(url)
            .header(header::CONTENT_TYPE, "application/pdf")
            .body(pdf)
            .send()
            .await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("PDF submission failed with {status}: {body}").into());
        }
        Ok(())
    }

    fn submission_url(&self, workflow_id: &str) -> Result<Url, Box<dyn Error>> {
        let mut url = self.api_url.clone();
        url.path_segments_mut()
            .map_err(|()| "SCEPA API URL cannot be a base URL")?
            .pop_if_empty()
            .push("pdfs")
            .push("submissions")
            .push(workflow_id);
        Ok(url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_identifier_is_one_encoded_api_path_segment() {
        let client = UploadClient::new("http://localhost:3000").unwrap();
        assert_eq!(
            client.submission_url("folder/paper 1").unwrap().as_str(),
            "http://localhost:3000/pdfs/submissions/folder%2Fpaper%201"
        );
    }
}
