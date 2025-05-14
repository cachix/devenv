use std::process::Command;

use devenv_eval_cache::{command, db};

#[tokio::main]
async fn main() -> Result<(), command::CommandError> {
    let database_url = "sqlite:nix-eval-cache.db";

    // Extract database path from URL
    let path = std::path::PathBuf::from(database_url.trim_start_matches("sqlite:"));

    // Connect to database and run migrations
    let db = devenv_cache_core::db::Database::new(path, &db::MIGRATIONS)
        .await
        .map_err(|e| {
            command::CommandError::Io(std::io::Error::new(std::io::ErrorKind::Other, e))
        })?;
    let pool = db.pool().clone();

    let mut cmd = Command::new("nix");
    cmd.args(["eval", ".#devenv.processes"]);

    let output = command::CachedCommand::new(&pool).output(&mut cmd).await?;
    println!("{}", String::from_utf8_lossy(&output.stdout));

    Ok(())
}
