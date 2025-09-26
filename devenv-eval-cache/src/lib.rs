pub mod command;
pub mod db;
pub mod internal_log;
pub mod op;

pub use command::{
    CachedCommand, EnvInputDesc, FileInputDesc, Input, Output, supports_eval_caching,
};

/// Integration tests for caching behavior with Nix evaluation.
///
/// These tests require the `integration-tests` feature flag and the `DEVENV_NIX`
/// environment variable pointing to a Nix installation directory.
///
/// These tests do *not* cover flake-related edge-cases.
/// For example, this will not catch path resolution issues due to evaluation
/// restrictions/deficiencies in flakes.
///
/// Such behaviours are best tested by devenv-run-tests.
/// See tests/eval-cache-*
///
/// To run these tests:
/// ```bash
/// DEVENV_NIX=/path/to/nix cargo test --features integration-tests
/// ```
///
/// The tests cover:
/// - `builtins.readFile` caching and dependency detection
/// - `builtins.readDir` caching and dependency detection  
/// - `builtins.getEnv` caching and dependency detection
/// - `builtins.pathExists` caching and dependency detection
/// - Cache invalidation when files or environment variables change
/// - Complex expressions with multiple dependencies
/// - Cache persistence across sessions
#[cfg(all(test, feature = "integration-tests"))]
mod integration_tests {
    use super::*;
    use std::env;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use tempfile::TempDir;

    fn get_nix_binary() -> Result<String, Box<dyn std::error::Error>> {
        match env::var("DEVENV_NIX") {
            Ok(path) => Ok(format!("{}/bin/nix", path)),
            Err(_) => Err(format!(
                "DEVENV_NIX environment variable not set. \
                Please set DEVENV_NIX to point to the store path of the custom Nix build. \
                Example: DEVENV_NIX=/nix/store/...-nix-devenv-2.30.0... cargo test --features integration-tests"
            ).into())
        }
    }

    fn create_test_file(dir: &Path, name: &str, content: &str) -> Result<PathBuf, std::io::Error> {
        let file_path = dir.join(name);
        std::fs::write(&file_path, content)?;
        Ok(file_path)
    }

    fn create_test_dir_with_files(
        dir: &Path,
        name: &str,
        files: &[(&str, &str)],
    ) -> Result<PathBuf, std::io::Error> {
        let dir_path = dir.join(name);
        std::fs::create_dir(&dir_path)?;
        for (file_name, content) in files {
            std::fs::write(dir_path.join(file_name), content)?;
        }
        Ok(dir_path)
    }

    async fn run_nix_eval_cached(
        pool: &sqlx::SqlitePool,
        expr: &str,
    ) -> Result<Output, Box<dyn std::error::Error>> {
        let nix_binary = get_nix_binary()?;
        let cached_cmd = CachedCommand::new(pool);
        let mut cmd = Command::new(nix_binary);
        cmd.args(&["eval", "--impure", "--expr", expr]);

        Ok(cached_cmd.output(&mut cmd).await?)
    }

    fn assert_file_dependency_detected(output: &Output, expected_path: &Path) {
        let found = output.inputs.iter().any(|input| {
            if let Input::File(f) = input {
                // Try canonicalization, but fall back to comparing parent directory + filename
                if let (Ok(expected_canonical), Ok(file_canonical)) =
                    (expected_path.canonicalize(), f.path.canonicalize())
                {
                    file_canonical == expected_canonical
                } else {
                    // For non-existent files, compare the resolved parent + filename
                    let expected_parent = expected_path
                        .parent()
                        .and_then(|p| p.canonicalize().ok())
                        .unwrap_or_else(|| {
                            expected_path
                                .parent()
                                .unwrap_or(Path::new(""))
                                .to_path_buf()
                        });
                    let expected_filename = expected_path.file_name().unwrap_or_default();

                    let file_parent = f
                        .path
                        .parent()
                        .and_then(|p| p.canonicalize().ok())
                        .unwrap_or_else(|| f.path.parent().unwrap_or(Path::new("")).to_path_buf());
                    let file_filename = f.path.file_name().unwrap_or_default();

                    expected_parent == file_parent && expected_filename == file_filename
                }
            } else {
                false
            }
        });
        assert!(
            found,
            "Expected file dependency not detected: {:?}. Found inputs: {:?}",
            expected_path, output.inputs
        );
    }

    fn assert_env_dependency_detected(output: &Output, expected_env: &str) {
        let found = output
            .inputs
            .iter()
            .any(|input| matches!(input, Input::Env(e) if e.name == expected_env));
        assert!(
            found,
            "Expected env dependency not detected: {}. Found inputs: {:?}",
            expected_env, output.inputs
        );
    }

    #[sqlx::test]
    async fn test_readfile_caching(
        pool: sqlx::SqlitePool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let test_file = create_test_file(temp_dir.path(), "sample.txt", "Hello, World!")?;

        let nix_expr = format!(r#"builtins.readFile "{}""#, test_file.display());

        // Run nix eval with caching
        let output = run_nix_eval_cached(&pool, &nix_expr).await?;

        // Verify the command executed successfully
        assert!(output.status.success(), "Nix eval command failed");

        // Verify file dependency was detected
        assert_file_dependency_detected(&output, &test_file);

        // Verify the output contains the file content
        let stdout_str = String::from_utf8(output.stdout)?;
        assert!(
            stdout_str.contains("Hello, World!"),
            "Output should contain file content"
        );

        Ok(())
    }

    #[sqlx::test]
    async fn test_readdir_caching(
        pool: sqlx::SqlitePool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let test_dir = create_test_dir_with_files(
            temp_dir.path(),
            "testdir",
            &[("file1.txt", "content1"), ("file2.txt", "content2")],
        )?;

        let nix_expr = format!(r#"builtins.readDir "{}""#, test_dir.display());

        // Run nix eval with caching
        let output = run_nix_eval_cached(&pool, &nix_expr).await?;

        // Verify the command executed successfully
        assert!(output.status.success(), "Nix eval command failed");

        // Verify directory dependency was detected
        assert_file_dependency_detected(&output, &test_dir);

        // Verify the output contains directory listing
        let stdout_str = String::from_utf8(output.stdout)?;
        assert!(
            stdout_str.contains("file1.txt"),
            "Output should contain file1.txt"
        );
        assert!(
            stdout_str.contains("file2.txt"),
            "Output should contain file2.txt"
        );

        Ok(())
    }

    #[sqlx::test]
    async fn test_getenv_caching(pool: sqlx::SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
        let test_env_var = "TEST_CACHE_VAR";
        let test_env_value = "test_value_12345";

        // Set test environment variable
        unsafe {
            env::set_var(test_env_var, test_env_value);
        }

        let nix_expr = format!(r#"builtins.getEnv "{}""#, test_env_var);

        // Run nix eval with caching
        let output = run_nix_eval_cached(&pool, &nix_expr).await?;

        // Verify the command executed successfully
        assert!(output.status.success(), "Nix eval command failed");

        // Verify env var dependency was detected
        assert_env_dependency_detected(&output, test_env_var);

        // Verify the output contains the env var value
        let stdout_str = String::from_utf8(output.stdout)?;
        assert!(
            stdout_str.contains(test_env_value),
            "Output should contain env var value"
        );

        // Clean up
        unsafe {
            env::remove_var(test_env_var);
        }

        Ok(())
    }

    #[sqlx::test]
    async fn test_pathexists_caching(
        pool: sqlx::SqlitePool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let existing_file = create_test_file(temp_dir.path(), "exists.txt", "I exist!")?;
        let nonexistent_file = temp_dir.path().join("does_not_exist.txt");

        // Test existing file
        let nix_expr = format!(r#"builtins.pathExists "{}""#, existing_file.display());
        let output = run_nix_eval_cached(&pool, &nix_expr).await?;

        assert!(output.status.success(), "Nix eval command failed");
        assert_file_dependency_detected(&output, &existing_file);

        let stdout_str = String::from_utf8(output.stdout)?;
        assert!(
            stdout_str.contains("true"),
            "Output should be true for existing file"
        );

        // Test non-existent file
        let nix_expr = format!(r#"builtins.pathExists "{}""#, nonexistent_file.display());
        let output = run_nix_eval_cached(&pool, &nix_expr).await?;

        assert!(output.status.success(), "Nix eval command failed");
        assert_file_dependency_detected(&output, &nonexistent_file);

        let stdout_str = String::from_utf8(output.stdout)?;
        assert!(
            stdout_str.contains("false"),
            "Output should be false for non-existent file"
        );

        Ok(())
    }

    #[sqlx::test]
    async fn test_cache_invalidation_on_file_change(
        pool: sqlx::SqlitePool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let test_file = temp_dir.path().join("mutable.txt");

        // Initial content
        std::fs::write(&test_file, "original content")?;
        let nix_expr = format!(r#"builtins.readFile "{}""#, test_file.display());

        // First run - should not hit cache (new command)
        let output1 = run_nix_eval_cached(&pool, &nix_expr).await?;
        assert!(output1.status.success(), "First nix eval failed");
        assert!(!output1.cache_hit, "First run should not hit cache");
        assert_file_dependency_detected(&output1, &test_file);

        let stdout1 = String::from_utf8(output1.stdout)?;
        assert!(
            stdout1.contains("original content"),
            "First run should contain original content"
        );

        // Second run - should hit cache (same file, same content)
        let output2 = run_nix_eval_cached(&pool, &nix_expr).await?;
        assert!(output2.status.success(), "Second nix eval failed");
        assert!(output2.cache_hit, "Second run should hit cache");

        // Modify file content and set mtime to ensure cache invalidation
        std::fs::write(&test_file, "modified content")?;

        // Set file mtime to current time + 1 second to ensure it's different
        let new_time = std::time::SystemTime::now() + std::time::Duration::from_secs(1);
        std::fs::File::open(&test_file)?.set_modified(new_time)?;

        // Third run - should invalidate cache (file changed)
        let output3 = run_nix_eval_cached(&pool, &nix_expr).await?;
        assert!(output3.status.success(), "Third nix eval failed");
        assert!(
            !output3.cache_hit,
            "Third run should not hit cache after file change"
        );

        let stdout3 = String::from_utf8(output3.stdout)?;
        assert!(
            stdout3.contains("modified content"),
            "Third run should contain modified content"
        );

        Ok(())
    }

    #[sqlx::test]
    async fn test_cache_invalidation_on_env_change(
        pool: sqlx::SqlitePool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let test_env_var = "TEST_CACHE_INVALIDATION_VAR";
        let nix_expr = format!(r#"builtins.getEnv "{}""#, test_env_var);

        // Set initial value
        unsafe {
            env::set_var(test_env_var, "initial_value");
        }

        // First run
        let output1 = run_nix_eval_cached(&pool, &nix_expr).await?;
        assert!(output1.status.success());
        assert!(!output1.cache_hit);
        assert_env_dependency_detected(&output1, test_env_var);

        // Second run - should hit cache
        let output2 = run_nix_eval_cached(&pool, &nix_expr).await?;
        assert!(output2.status.success());
        assert!(output2.cache_hit);

        // Change environment variable
        unsafe {
            env::set_var(test_env_var, "changed_value");
        }

        // Third run - should invalidate cache
        let output3 = run_nix_eval_cached(&pool, &nix_expr).await?;
        assert!(output3.status.success());
        assert!(!output3.cache_hit);

        let stdout3 = String::from_utf8(output3.stdout)?;
        assert!(stdout3.contains("changed_value"));

        // Clean up
        unsafe {
            env::remove_var(test_env_var);
        }

        Ok(())
    }

    #[sqlx::test]
    async fn test_evaluated_file_caching(
        pool: sqlx::SqlitePool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let nix_file = create_test_file(temp_dir.path(), "test.nix", r#""hello from nix file""#)?;

        let nix_expr = format!(r#"import {}"#, nix_file.display());

        // Run nix eval with caching
        let output = run_nix_eval_cached(&pool, &nix_expr).await?;

        // Verify the command executed successfully
        assert!(output.status.success(), "Nix eval command failed");

        // Verify file dependency was detected (the imported nix file)
        assert_file_dependency_detected(&output, &nix_file);

        // Verify the output contains the expected content
        let stdout_str = String::from_utf8(output.stdout)?;
        assert!(
            stdout_str.contains("hello from nix file"),
            "Output should contain nix file content"
        );

        Ok(())
    }

    #[sqlx::test]
    async fn test_complex_dependency_tracking(
        pool: sqlx::SqlitePool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;

        // Create multiple test files
        let config_file =
            create_test_file(temp_dir.path(), "config.json", r#"{"version": "1.0"}"#)?;
        let data_file = create_test_file(temp_dir.path(), "data.txt", "important data")?;
        let test_dir = create_test_dir_with_files(
            temp_dir.path(),
            "subdir",
            &[("nested.txt", "nested content")],
        )?;

        // Set test environment variable
        unsafe {
            env::set_var("COMPLEX_TEST_VAR", "complex_value");
        }

        // Create a complex Nix expression that uses multiple operations
        let nix_expr = format!(
            r#"{{
                config = builtins.fromJSON (builtins.readFile "{}");
                data = builtins.readFile "{}";
                dirContents = builtins.readDir "{}";
                envVar = builtins.getEnv "COMPLEX_TEST_VAR";
                configExists = builtins.pathExists "{}";
            }}"#,
            config_file.display(),
            data_file.display(),
            test_dir.display(),
            config_file.display()
        );

        // Run nix eval with caching
        let output = run_nix_eval_cached(&pool, &nix_expr).await?;

        // Verify the command executed successfully
        assert!(output.status.success(), "Complex nix eval command failed");

        // Verify all dependencies were detected
        assert_file_dependency_detected(&output, &config_file);
        assert_file_dependency_detected(&output, &data_file);
        assert_file_dependency_detected(&output, &test_dir);
        assert_env_dependency_detected(&output, "COMPLEX_TEST_VAR");

        // Clean up
        unsafe {
            env::remove_var("COMPLEX_TEST_VAR");
        }

        Ok(())
    }

    #[sqlx::test]
    async fn test_cache_persistence_across_sessions(
        pool: sqlx::SqlitePool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let test_file = create_test_file(temp_dir.path(), "persistent.txt", "persistent content")?;

        let nix_expr = format!(r#"builtins.readFile "{}""#, test_file.display());

        // First session - create cache entry
        let output1 = run_nix_eval_cached(&pool, &nix_expr).await?;
        assert!(output1.status.success());
        assert!(!output1.cache_hit);

        // Second session - should hit cache from database
        let output2 = run_nix_eval_cached(&pool, &nix_expr).await?;
        assert!(output2.status.success());
        assert!(output2.cache_hit);

        // Verify both outputs are identical
        assert_eq!(output1.stdout, output2.stdout);

        Ok(())
    }
}
