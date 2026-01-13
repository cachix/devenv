use std::path::PathBuf;

/// Configuration for the shell manager
pub struct Config {
    /// Files to watch for changes (relative to cwd)
    pub watch_files: Vec<PathBuf>,
}

impl Config {
    pub fn new(watch_files: Vec<PathBuf>) -> Self {
        Self { watch_files }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_new() {
        let config = Config::new(vec![PathBuf::from("test.nix")]);
        assert_eq!(config.watch_files.len(), 1);
    }

    #[test]
    fn test_config_new_empty_files() {
        let config = Config::new(vec![]);
        assert!(config.watch_files.is_empty());
    }

    #[test]
    fn test_config_multiple_files() {
        let config = Config::new(vec![PathBuf::from("a.nix"), PathBuf::from("b.nix")]);
        assert_eq!(config.watch_files.len(), 2);
    }
}
