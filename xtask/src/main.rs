use clap::Parser;
use miette::Result;
use xtask::{manpage, shell_completion};

#[derive(clap::Parser)]
struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    GenerateManpages {
        #[clap(
            long,
            value_parser,
            value_hint = clap::ValueHint::DirPath,
            default_value_os_t = manpage::default_out_dir()
        )]
        out_dir: std::path::PathBuf,
    },
    GenerateShellCompletion {
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
        Command::GenerateManpages { out_dir } => manpage::generate(out_dir),
        Command::GenerateShellCompletion { shell, out_dir } => {
            shell_completion::generate(shell, out_dir)
        }
    }
}
