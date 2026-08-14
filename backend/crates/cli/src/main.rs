use std::{error::Error, path::PathBuf};

use clap::{Parser, Subcommand};

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

fn main() -> Result<(), Box<dyn Error>> {
    match Cli::parse().command {
        Command::Single { identifier, pdf } => upload_pdf(pdf, identifier),
        Command::Batch { directory } => upload_batch(directory),
    }
}

fn upload_pdf(_pdf: PathBuf, _identifier: Option<String>) -> Result<(), Box<dyn Error>> {
    todo!()
}

fn upload_batch(_directory: PathBuf) -> Result<(), Box<dyn Error>> {
    todo!()
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
