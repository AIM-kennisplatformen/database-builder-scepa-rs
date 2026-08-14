use std::{env, error::Error, path::PathBuf, time::Duration};

use clap::{Parser, Subcommand};
use scepa::{
    operations::submit_pipeline,
    pipeline::{
        PipelineService,
        garage::{GarageClient, GaragePipelineService, PostgresPdfStore},
    },
    postgres::PostgresReviewStore,
};

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
    /// Submits a single PDF to the document pipeline.
    Single {
        /// Overrides the PDF file stem used as the Restate workflow identifier.
        #[arg(long)]
        identifier: Option<String>,
        /// PDF to submit.
        pdf: PathBuf,
    },
    /// Submits every PDF in a directory to the document pipeline.
    Batch {
        /// Directory containing PDFs to submit.
        directory: PathBuf,
    },
}

struct Runtime {
    http_client: reqwest::Client,
    restate_ingress_url: String,
    garage_pipeline: GaragePipelineService,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    let runtime = Runtime::from_env().await?;
    match cli.command {
        Command::Single { identifier, pdf } => {
            submit_file(&runtime, &pdf, identifier.as_deref()).await
        }
        Command::Batch { directory } => submit_batch(&runtime, &directory).await,
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
            http_client,
            restate_ingress_url: env::var("RESTATE_INGRESS_URL")
                .unwrap_or_else(|_| "http://localhost:8080".into()),
            garage_pipeline,
        })
    }
}

async fn submit_batch(runtime: &Runtime, directory: &PathBuf) -> Result<(), Box<dyn Error>> {
    let mut files = pdf_files(directory).await?;
    files.sort();
    if files.is_empty() {
        return Err(format!("{} contains no PDF files", directory.display()).into());
    }
    for file in files {
        submit_file(runtime, &file, None).await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn parses_only_single_and_batch_commands() {
        let command_names = Cli::command()
            .get_subcommands()
            .map(|command| command.get_name().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(command_names, ["single", "batch"]);

        let cli = Cli::try_parse_from([
            "scepa-cli",
            "single",
            "--identifier",
            "paper-debug",
            "paper.pdf",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Command::Single {
                identifier: Some(identifier),
                pdf
            } if identifier == "paper-debug" && pdf == PathBuf::from("paper.pdf")
        ));

        let cli = Cli::try_parse_from(["scepa-cli", "batch", "papers"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Batch { directory } if directory == PathBuf::from("papers")
        ));

        assert!(Cli::try_parse_from(["scepa-cli", "pipeline", "run", "paper.pdf"]).is_err());
        assert!(Cli::try_parse_from(["scepa-cli", "artifact", "patch", "42", "fix.pdf"]).is_err());
    }
}
