//! `devenv direnvrc`: print the bundled direnv integration script.

use miette::Result;

pub fn run() -> Result<()> {
    print!("{}", *crate::DIRENVRC);
    Ok(())
}
