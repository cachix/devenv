//! `devenv direnvrc`: print the bundled direnv integration script.

pub fn run() -> Result(()) {
    print!("{}", *crate::DIRENVRC);
    Ok(())
}
