//! `devenv direnvrc`: print the bundled direnv integration script.

pub fn run() {
    print!("{}", *crate::DIRENVRC);
}
