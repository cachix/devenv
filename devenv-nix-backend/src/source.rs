use std::fs;
use std::path::{Component, Path, PathBuf};

use devenv_core::config::Input;
use devenv_core::{NixSettings, StoreSettings};
use miette::{IntoDiagnostic, Result, WrapErr, bail, miette};
use serde_json::Value;

use crate::anyhow_ext::AnyhowToMiette;
use crate::{backend, lock};

#[derive(Debug, PartialEq, Eq)]
pub struct MaterializedSource {
    repository_root: PathBuf,
    directory: Option<PathBuf>,
}

impl MaterializedSource {
    pub fn repository_root(&self) -> &Path {
        &self.repository_root
    }

    pub fn directory(&self) -> &Path {
        self.directory.as_deref().unwrap_or(&self.repository_root)
    }
}

pub fn materialize_source(
    nix_settings: &NixSettings,
    root: &Path,
    lock_file_path: &Path,
    source_input: &Input,
) -> Result<MaterializedSource> {
    let store_settings = StoreSettings::default();
    let _gc_registration = backend::init_nix(nix_settings, &store_settings)
        .wrap_err("Failed to initialize Nix for source resolution")?;
    let store = backend::open_store(&store_settings)
        .wrap_err("Failed to open the Nix store for source resolution")?;
    let (flake_settings, fetchers_settings) =
        backend::build_settings().wrap_err("Failed to build Nix source settings")?;
    let mut eval_state =
        lock::build_eval_state(&store, root, &flake_settings, nix_settings.refresh_fetchers)
            .wrap_err("Failed to build the Nix source evaluator")?;

    let source_lock = crate::lock_source_input(
        &eval_state,
        &fetchers_settings,
        &flake_settings,
        root,
        lock_file_path,
        source_input,
        nix_settings.refresh_fetchers,
    )
    .to_miette()
    .wrap_err("Failed to lock the source input")?;
    let lock_json = source_lock
        .to_string()
        .to_miette()
        .wrap_err("Failed to serialize the source lock")?;
    let (locked_attributes, dir) = locked_source_attributes(&lock_json)?;

    let locked_json = serde_json::to_string(&locked_attributes)
        .into_diagnostic()
        .wrap_err("Locked source attributes are invalid")?;
    let locked_nix_string = ser_nix::to_string(&locked_json)
        .into_diagnostic()
        .wrap_err("Failed to serialize locked source attributes for Nix")?;
    let expression =
        format!("toString (builtins.fetchTree (builtins.fromJSON {locked_nix_string})).outPath");
    let base = root
        .to_str()
        .ok_or_else(|| miette!("Source root path contains invalid UTF-8"))?;
    let fetched_value = eval_state
        .eval_from_string(&expression, base)
        .to_miette()
        .wrap_err("Nix could not fetch the locked source")?;
    let fetched_root = eval_state
        .realise_string(&fetched_value, false)
        .to_miette()
        .wrap_err("Nix could not fetch the locked source")?;

    select_source_directory(Path::new(&fetched_root.s), dir.as_deref())
}

fn locked_source_attributes(lock_json: &str) -> Result<(Value, Option<String>)> {
    let lock: Value = serde_json::from_str(lock_json)
        .into_diagnostic()
        .wrap_err("Locked source attributes are invalid: the lock is not valid JSON")?;
    let nodes = lock
        .get("nodes")
        .and_then(Value::as_object)
        .ok_or_else(|| miette!("Source lock node is absent: the lock has no nodes"))?;
    let root_name = lock
        .get("root")
        .and_then(Value::as_str)
        .ok_or_else(|| miette!("Source lock node is absent: the lock has no root node"))?;
    let root_node = nodes
        .get(root_name)
        .ok_or_else(|| miette!("Source lock node is absent: root node '{root_name}' is missing"))?;
    let source_node_name = root_node
        .get("inputs")
        .and_then(|inputs| inputs.get("from"))
        .and_then(Value::as_str)
        .ok_or_else(|| miette!("Source lock node is absent: input 'from' is missing"))?;
    let source_node = nodes.get(source_node_name).ok_or_else(|| {
        miette!("Source lock node is absent: node '{source_node_name}' is missing")
    })?;
    let mut locked = source_node
        .get("locked")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| {
            miette!(
                "Locked source attributes are invalid: node '{source_node_name}' has no locked attribute set"
            )
        })?;
    let dir = match locked.remove("dir") {
        Some(Value::String(dir)) => Some(dir),
        Some(_) => {
            bail!(
                "Locked source attributes are invalid: node '{source_node_name}' has a non-string dir"
            )
        }
        None => None,
    };
    Ok((Value::Object(locked), dir))
}

fn select_source_directory(fetched_root: &Path, dir: Option<&str>) -> Result<MaterializedSource> {
    let fetched_root = fs::canonicalize(fetched_root)
        .into_diagnostic()
        .wrap_err_with(|| {
            format!(
                "The fetched source root does not exist: {}",
                fetched_root.display()
            )
        })?;
    let Some(dir) = dir else {
        return Ok(MaterializedSource {
            repository_root: fetched_root,
            directory: None,
        });
    };

    let mut relative_dir = PathBuf::new();
    for component in Path::new(dir).components() {
        match component {
            Component::Normal(component) => relative_dir.push(component),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("The requested dir '{dir}' escapes the source root")
            }
        }
    }

    let selected = fetched_root.join(relative_dir);
    let selected = fs::canonicalize(&selected)
        .into_diagnostic()
        .wrap_err_with(|| {
            format!(
                "The selected source directory does not exist: {}",
                selected.display()
            )
        })?;
    if selected != fetched_root && !selected.starts_with(&fetched_root) {
        bail!("The requested dir '{dir}' escapes the source root");
    }
    if !selected.is_dir() {
        bail!(
            "The selected source path is not a directory: {}",
            selected.display()
        );
    }
    let directory = (selected != fetched_root).then_some(selected);
    Ok(MaterializedSource {
        repository_root: fetched_root,
        directory,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_source_root_without_dir() {
        let source = tempfile::tempdir().unwrap();

        let selected = select_source_directory(source.path(), None).unwrap();
        let root = fs::canonicalize(source.path()).unwrap();

        assert_eq!(selected.repository_root(), root);
        assert_eq!(selected.directory(), root);
    }

    #[test]
    fn selects_source_child_from_dir() {
        let source = tempfile::tempdir().unwrap();
        let child = source.path().join("profiles").join("rails");
        fs::create_dir_all(&child).unwrap();

        let selected = select_source_directory(source.path(), Some("profiles/rails")).unwrap();

        assert_eq!(
            selected.repository_root(),
            fs::canonicalize(source.path()).unwrap()
        );
        assert_eq!(selected.directory(), fs::canonicalize(child).unwrap());
    }

    #[test]
    fn rejects_file_from_dir() {
        let source = tempfile::tempdir().unwrap();
        let file = source.path().join("devenv.yaml");
        fs::write(&file, "inputs: {}").unwrap();

        let error = select_source_directory(source.path(), Some("devenv.yaml")).unwrap_err();

        assert_eq!(
            error.to_string(),
            format!(
                "The selected source path is not a directory: {}",
                fs::canonicalize(file).unwrap().display()
            )
        );
    }

    #[test]
    fn rejects_parent_directory_in_dir() {
        let source = tempfile::tempdir().unwrap();

        let error = select_source_directory(source.path(), Some("../outside")).unwrap_err();

        assert_eq!(
            error.to_string(),
            "The requested dir '../outside' escapes the source root"
        );
    }

    #[test]
    fn reports_missing_source_lock_input() {
        let lock_json = r#"{"nodes":{"root":{"inputs":{}}},"root":"root"}"#;

        let error = locked_source_attributes(lock_json).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Source lock node is absent: input 'from' is missing"
        );
    }
}
