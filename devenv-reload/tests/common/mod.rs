use devenv_reload::{BuildContext, BuildError, CommandBuilder, ShellBuilder};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};
use tempfile::TempDir;

/// Create a temporary directory with specified files
pub fn create_temp_dir_with_files(files: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new().expect("create temp dir");
    for (name, content) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dirs");
        }
        File::create(&path)
            .expect("create file")
            .write_all(content.as_bytes())
            .expect("write content");
    }
    dir
}

/// Wait with timeout for a condition
#[allow(dead_code)]
pub fn wait_for<F>(timeout: Duration, mut condition: F) -> bool
where
    F: FnMut() -> bool,
{
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

/// Modify a file with new content
pub fn modify_file(path: &Path, content: &str) {
    let mut file = File::create(path).expect("open file");
    file.write_all(content.as_bytes()).expect("write");
    file.sync_all().expect("sync");
}

/// Simple shell builder for tests that runs a given command
pub struct TestShellBuilder {
    pub command: String,
    pub args: Vec<String>,
}

impl TestShellBuilder {
    pub fn new(command: &str) -> Self {
        Self {
            command: command.to_string(),
            args: vec![],
        }
    }

    pub fn with_args(mut self, args: &[&str]) -> Self {
        self.args = args.iter().map(|s| s.to_string()).collect();
        self
    }

    #[allow(dead_code)]
    pub fn echo(message: &str) -> Self {
        Self::new("sh").with_args(&["-c", &format!("echo '{}'", message)])
    }

    #[allow(dead_code)]
    pub fn sleep(seconds: u32) -> Self {
        Self::new("sleep").with_args(&[&seconds.to_string()])
    }
}

impl ShellBuilder for TestShellBuilder {
    fn build(&self, _ctx: &BuildContext) -> Result<CommandBuilder, BuildError> {
        let mut cmd = CommandBuilder::new(&self.command);
        for arg in &self.args {
            cmd.arg(arg);
        }
        Ok(cmd)
    }
}
