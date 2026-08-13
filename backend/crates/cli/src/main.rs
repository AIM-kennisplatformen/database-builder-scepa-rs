use std::{env, error::Error, path::PathBuf, time::Duration};

use clap::{Args, Parser, Subcommand};
use scepa::{
    operations::{PipelinePart, run_artifact_operation, submit_pipeline},
    pipeline::{
        PipelineService,
        garage::{GarageClient, GaragePipelineService, PostgresPdfStore},
    },
    postgres::PostgresReviewStore,
};

#[derive(Debug, Parser)]
#[command(name = "scepa-cli", about = "SCEPA pipeline command-line interface")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Runs or inspects the document pipeline.
    Pipeline(PipelineArgs),
    /// Manages persisted review artifacts.
    Artifact(ArtifactArgs),
}

#[derive(Debug, Args)]
struct PipelineArgs {
    #[command(subcommand)]
    command: PipelineCommand,
}

#[derive(Debug, Subcommand)]
enum PipelineCommand {
    /// Runs a part of the composite Grobid + TEI service on a stored artifact.
    Grobid {
        /// Pipeline part: input-validation, output-validation, or execute.
        part: PipelinePart,
        /// Existing review artifact ID.
        identifier: i64,
    },
    /// Submits one PDF, or every PDF in a directory with `run batch <dir>`.
    Run {
        /// Overrides the PDF file stem used as the Restate workflow identifier.
        #[arg(long)]
        identifier: Option<String>,
        #[arg(required = true, num_args = 1..=2)]
        target: Vec<PathBuf>,
    },
}

#[derive(Debug, Args)]
struct ArtifactArgs {
    #[command(subcommand)]
    command: ArtifactCommand,
}

#[derive(Debug, Subcommand)]
enum ArtifactCommand {
    /// Replaces the artifact of a pending validation failure.
    Patch {
        identifier: i64,
        replacement: PathBuf,
        #[arg(long)]
        content_type: Option<String>,
    },
}

struct Runtime {
    review_store: PostgresReviewStore,
    http_client: reqwest::Client,
    grobid_url: String,
    restate_ingress_url: String,
    garage_pipeline: GaragePipelineService,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    let runtime = Runtime::from_env().await?;
    match cli.command {
        Command::Pipeline(args) => run_pipeline_command(&runtime, args.command).await,
        Command::Artifact(args) => run_artifact_command(&runtime, args.command).await,
    }
}

impl Runtime {
    async fn from_env() -> Result<Self, Box<dyn Error>> {
        let database_url = env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://scepa:scepa@localhost:5432/scepa".into());
        let review_store = PostgresReviewStore::connect_lazy(&database_url)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        review_store
            .migrate()
            .await
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(600))
            .build()?;
        let garage = GarageClient::new(
            http_client.clone(),
            &env::var("GARAGE_ENDPOINT").unwrap_or_else(|_| "http://localhost:3900".into()),
            env::var("GARAGE_REGION").unwrap_or_else(|_| "garage".into()),
            env::var("GARAGE_ACCESS_KEY")
                .unwrap_or_else(|_| "GK00000000000000000000000000000000".into()),
            env::var("GARAGE_SECRET_KEY").unwrap_or_else(|_| {
                "0000000000000000000000000000000000000000000000000000000000000000".into()
            }),
        )
        .map_err(|error| std::io::Error::other(error.to_string()))?;
        let garage_pipeline = GaragePipelineService::new(
            PostgresPdfStore::new(review_store.pool().clone()),
            garage,
            env::var("GARAGE_BUCKET").unwrap_or_else(|_| "scepa-pdfs".into()),
            review_store.clone(),
        );
        Ok(Self {
            review_store,
            http_client,
            grobid_url: env::var("GROBID_URL").unwrap_or_else(|_| "http://localhost:8070".into()),
            restate_ingress_url: env::var("RESTATE_INGRESS_URL")
                .unwrap_or_else(|_| "http://localhost:8080".into()),
            garage_pipeline,
        })
    }
}

async fn run_pipeline_command(
    runtime: &Runtime,
    command: PipelineCommand,
) -> Result<(), Box<dyn Error>> {
    match command {
        PipelineCommand::Grobid { part, identifier } => {
            let response = run_artifact_operation(
                &runtime.review_store,
                None,
                runtime.http_client.clone(),
                &runtime.grobid_url,
                part,
                identifier,
                None,
            )
            .await?;
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
        PipelineCommand::Run { identifier, target } => match target.as_slice() {
            [pdf] => submit_file(runtime, pdf, identifier.as_deref()).await?,
            [batch, directory] if batch.as_os_str() == "batch" => {
                if identifier.is_some() {
                    return Err("--identifier is only valid for a single PDF".into());
                }
                let mut files = pdf_files(directory).await?;
                files.sort();
                if files.is_empty() {
                    return Err(format!("{} contains no PDF files", directory.display()).into());
                }
                for file in files {
                    submit_file(runtime, &file, None).await?;
                }
            }
            _ => return Err("expected `pipeline run <pdf>` or `pipeline run batch <dir>`".into()),
        },
    }
    Ok(())
}

async fn submit_file(
    runtime: &Runtime,
    path: &PathBuf,
    identifier: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    let identifier = match identifier {
        Some(identifier) if !identifier.is_empty() => identifier,
        Some(_) => return Err("--identifier must not be empty".into()),
        None => path
            .file_stem()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("{} has no usable file stem", path.display()))?,
    };
    let pdf = tokio::fs::read(path).await?;
    let stored = runtime
        .garage_pipeline
        .execute(identifier, &pdf)
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?
        .into_output(|_| {});
    runtime
        .garage_pipeline
        .metadata()
        .link_workflow(identifier, &stored.pdf_hash)
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let response = submit_pipeline(
        &runtime.http_client,
        &runtime.restate_ingress_url,
        identifier,
        &stored.pdf_hash,
    )
    .await?;
    let status = response.status();
    let body = response.text().await?;
    println!("{identifier}: {status} {body}");
    if !status.is_success() {
        return Err(format!("pipeline submission failed for {identifier}").into());
    }
    Ok(())
}

async fn pdf_files(directory: &PathBuf) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut entries = tokio::fs::read_dir(directory).await?;
    let mut files = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if entry.file_type().await?.is_file()
            && path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("pdf"))
        {
            files.push(path);
        }
    }
    Ok(files)
}

async fn run_artifact_command(
    runtime: &Runtime,
    command: ArtifactCommand,
) -> Result<(), Box<dyn Error>> {
    match command {
        ArtifactCommand::Patch {
            identifier,
            replacement,
            content_type,
        } => {
            let bytes = tokio::fs::read(&replacement).await?;
            let content_type = content_type.unwrap_or_else(|| infer_content_type(&replacement));
            let updated = runtime
                .review_store
                .patch_validation_artifact(identifier, &content_type, &bytes)
                .await
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            if !updated {
                return Err("artifact is missing, resolved, or is not a validation failure".into());
            }
            println!(
                "patched artifact {identifier} with {} bytes ({content_type})",
                bytes.len()
            );
        }
    }
    Ok(())
}

fn infer_content_type(path: &std::path::Path) -> String {
    match path.extension().and_then(|value| value.to_str()) {
        Some(extension) if extension.eq_ignore_ascii_case("pdf") => "application/pdf",
        Some(extension) if extension.eq_ignore_ascii_case("json") => "application/json",
        Some(extension)
            if extension.eq_ignore_ascii_case("xml") || extension.eq_ignore_ascii_case("tei") =>
        {
            "application/tei+xml"
        }
        _ => "application/octet-stream",
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_requested_pipeline_commands() {
        let cli =
            Cli::try_parse_from(["scepa-cli", "pipeline", "grobid", "output-validation", "42"])
                .unwrap();
        assert!(matches!(
            cli.command,
            Command::Pipeline(PipelineArgs {
                command: PipelineCommand::Grobid {
                    part: PipelinePart::OutputValidation,
                    identifier: 42
                }
            })
        ));

        Cli::try_parse_from(["scepa-cli", "pipeline", "run", "batch", "papers"]).unwrap();
        Cli::try_parse_from(["scepa-cli", "pipeline", "run", "paper.pdf"]).unwrap();
    }
}
