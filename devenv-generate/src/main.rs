use clap::{Parser, crate_version};
use devenv::{
    default_system,
    log::{self, LogFormat},
};
use miette::{IntoDiagnostic, Result, bail};
use similar::{ChangeTag, TextDiff};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

#[derive(Parser, Debug)]
#[command(
    name = "devenv-generate",
    about = "Generate devenv.yaml and devenv.nix using AI"
)]
struct Cli {
    #[arg(num_args=0.., trailing_var_arg = true)]
    description: Vec<String>,

    #[clap(long, default_value = "https://devenv.new/api/generate")]
    host: String,

    #[arg(
        long,
        help = "Paths to exclude during generation.",
        value_name = "PATH"
    )]
    exclude: Vec<PathBuf>,

    // https://consoledonottrack.com/
    #[clap(long, env = "DO_NOT_TRACK", action = clap::ArgAction::SetTrue)]
    disable_telemetry: bool,

    #[arg(
        short = 'V',
        long,
        global = true,
        help = "Print version information",
        long_help = "Print version information and exit"
    )]
    pub version: bool,

    #[arg(short, long, global = true, default_value_t = default_system())]
    pub system: String,

    #[arg(short, long, global = true, help = "Enable additional debug logs.")]
    verbose: bool,

    #[arg(
        short,
        long,
        global = true,
        conflicts_with = "verbose",
        help = "Silence all logs"
    )]
    pub quiet: bool,

    #[arg(
        long,
        global = true,
        help = "Configure the output format of the logs.",
        default_value_t,
        value_enum
    )]
    pub log_format: LogFormat,
}

#[derive(serde::Deserialize)]
struct GenerateResponse {
    devenv_nix: String,
    devenv_yaml: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.version {
        println!("devenv {} ({})", crate_version!(), cli.system);
        return Ok(());
    }

    let level = if cli.verbose {
        log::Level::Debug
    } else if cli.quiet {
        log::Level::Silent
    } else {
        log::Level::default()
    };

    log::init_tracing(level, cli.log_format);

    let description = if !cli.description.is_empty() {
        Some(cli.description.join(" "))
    } else {
        None
    };

    let client = reqwest::Client::builder()
        .use_preconfigured_tls(http_client_tls::tls_config())
        .build()
        .expect("Failed to create reqwest client");
    let mut request = client
        .post(&cli.host)
        .query(&[("disable_telemetry", cli.disable_telemetry)])
        .header(reqwest::header::USER_AGENT, crate_version!());

    let (asyncwriter, asyncreader) = tokio::io::duplex(256 * 1024);
    let streamreader = tokio_util::io::ReaderStream::new(asyncreader);

    let (body_sender, body) = match description {
        Some(desc) => {
            request = request.query(&[("q", desc)]);
            (None, None)
        }
        None => {
            let git_output = std::process::Command::new("git")
                .args(["ls-files", "-z"])
                .output()
                .map_err(|_| miette::miette!("Failed to get list of files from git ls-files"))?;

            let files = String::from_utf8_lossy(&git_output.stdout)
                .split('\0')
                .filter(|s| !s.is_empty())
                .filter(|s| !binaryornot::is_binary(s).unwrap_or(false))
                .map(PathBuf::from)
                .collect::<Vec<_>>();

            if files.is_empty() {
                warn!("No files found. Are you in a git repository?");
                return Ok(());
            }

            if let Ok(stderr) = String::from_utf8(git_output.stderr)
                && !stderr.is_empty()
            {
                warn!("{}", &stderr);
            }

            let body = reqwest::Body::wrap_stream(streamreader);

            request = request
                .body(body)
                .header(reqwest::header::CONTENT_TYPE, "application/x-tar");

            (Some(tokio_tar::Builder::new(asyncwriter)), Some(files))
        }
    };

    info!("Generating devenv.nix and devenv.yaml, this should take about a minute ...");

    let response_future = request.send();

    let tar_task = async {
        if let (Some(mut builder), Some(files)) = (body_sender, body) {
            for path in files {
                if path.is_file() && !cli.exclude.iter().any(|exclude| path.starts_with(exclude)) {
                    builder.append_path(&path).await?;
                }
            }
            builder.finish().await?;
        }
        Ok::<(), std::io::Error>(())
    };

    let (response, _) = tokio::join!(response_future, tar_task);

    let response = response.into_diagnostic()?;
    let status = response.status();
    if !status.is_success() {
        let error_text = &response
            .text()
            .await
            .unwrap_or_else(|_| "No error details available".to_string());
        bail!(
            "Failed to generate (HTTP {}): {}",
            &status.as_u16(),
            match serde_json::from_str::<serde_json::Value>(error_text) {
                Ok(json) => json["message"]
                    .as_str()
                    .map(String::from)
                    .unwrap_or_else(|| error_text.clone()),
                Err(_) => error_text.clone(),
            }
        );
    }

    let response_json: GenerateResponse = response.json().await.expect("Failed to parse JSON.");

    confirm_overwrite(Path::new("devenv.nix"), response_json.devenv_nix)?;
    confirm_overwrite(Path::new("devenv.yaml"), response_json.devenv_yaml)?;

    info!(
        "{}",
        indoc::formatdoc!("
          Generated devenv.nix and devenv.yaml 🎉

          Treat these as templates and open an issue at https://github.com/cachix/devenv/issues if you think we can do better!

          Start by running:

            $ devenv shell
        "));
    Ok(())
}

fn confirm_overwrite(file: &Path, contents: String) -> Result<()> {
    if std::fs::metadata(file).is_ok() {
        // first output the old version and propose new changes
        let before = std::fs::read_to_string(file).expect("Failed to read file");

        let diff = TextDiff::from_lines(&before, &contents);

        println!("\nChanges that will be made to {}:", file.to_string_lossy());
        for change in diff.iter_all_changes() {
            let sign = match change.tag() {
                ChangeTag::Delete => "\x1b[31m-\x1b[0m",
                ChangeTag::Insert => "\x1b[32m+\x1b[0m",
                ChangeTag::Equal => " ",
            };
            print!("{sign}{change}");
        }

        let confirm = dialoguer::Confirm::new()
            .with_prompt(format!(
                "{} already exists. Do you want to overwrite it?",
                file.to_string_lossy()
            ))
            .interact()
            .into_diagnostic()?;

        if confirm {
            std::fs::write(file, contents).into_diagnostic()?;
        }
    } else {
        std::fs::write(file, contents).into_diagnostic()?;
    }
    Ok(())
}
