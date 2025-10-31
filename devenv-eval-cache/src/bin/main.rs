use std::process::Command;

use devenv_eval_cache::{command, db};

#[tokio::main]
async fn main() -> Result<(), command::CommandError> {
    let database_url = "sqlite:nix-eval-cache.db";

    // Extract database path from URL
    let path = std::path::PathBuf::from(database_url.trim_start_matches("sqlite:"));

    // Get migrations directory and connect to database
    let migrations_dir = db::migrations_dir();
    let db = devenv_cache_core::db::Database::new(path, &migrations_dir)
        .await
        .map_err(|e| command::CommandError::Io(std::io::Error::other(e)))?;
    let conn = db
        .connect()
        .map_err(|e| command::CommandError::Io(std::io::Error::other(e)))?;

    let mut cmd = Command::new("nix");
    cmd.args(["eval", ".#devenv.processes"]);

    let output = command::NixCommand::new(&conn).output(&mut cmd).await?;
    println!("{}", String::from_utf8_lossy(&output.stdout));

    Ok(())
}
