use clap::Parser;
use miette::Result;
use xtask::{config_docs, manpage, shell_completion};

#[derive(clap::Parser)]
struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    #[command(name = "generate-json-schema")]
    JsonSchema {
        #[clap(
            long,
            value_parser,
            value_hint = clap::ValueHint::FilePath,
            default_value_os_t = config_docs::default_json_schema_path()
        )]
        output: std::path::PathBuf,
    },
    #[command(name = "generate-yaml-options-doc")]
    YamlOptionsDoc {
        #[clap(
            long,
            value_parser,
            value_hint = clap::ValueHint::FilePath,
            default_value_os_t = config_docs::default_yaml_options_path()
        )]
        output: std::path::PathBuf,
    },
    #[command(name = "generate-manpages")]
    Manpages {
        #[clap(
            long,
            value_parser,
            value_hint = clap::ValueHint::DirPath,
            default_value_os_t = manpage::default_out_dir()
        )]
        out_dir: std::path::PathBuf,
    },
    #[command(name = "generate-shell-completion")]
    ShellCompletion {
        #[clap(value_enum)]
        shell: clap_complete::Shell,

        #[clap(
            long,
            value_parser,
            value_hint = clap::ValueHint::DirPath,
            default_value_os_t = shell_completion::default_out_dir()
        )]
        out_dir: std::path::PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::JsonSchema { output } => config_docs::generate_json_schema(output),
        Command::YamlOptionsDoc { output } => config_docs::generate_yaml_options_doc(output),
        Command::Manpages { out_dir } => manpage::generate(out_dir),
        Command::ShellCompletion { shell, out_dir } => shell_completion::generate(shell, out_dir),
    }
}
