//! `devenv version`: print the devenv version, build revision, and target system.
//!
//! Output should match `devenv --version`.

pub fn run() {
    println!("devenv {}", env!("DEVENV_VERSION_STRING"));
}
