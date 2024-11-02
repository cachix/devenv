use std::process::Command;

use devenv_eval_cache::{command, db};

#[tokio::main]
async fn main() -> Result<(), command::CommandError> {
    let database_url = "sqlite:nix-eval-cache.db";
    let pool = db::setup_db(database_url).await?;

    let mut cmd = Command::new("nix");
    cmd.args(["eval", ".#devenv.processes"]);

    let output = command::CachedCommand::new(&pool).output(&mut cmd).await?;
    println!("{}", String::from_utf8_lossy(&output.stdout));

    Ok(())
}
