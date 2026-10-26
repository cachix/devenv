use std::fs;
use std::path::{Component, Path, PathBuf};

use miette::{IntoDiagnostic, Result, WrapErr, bail};

#[derive(Debug, PartialEq, Eq)]
pub struct MaterializedSource {
    repository_root: PathBuf,
    path: PathBuf,
}

impl MaterializedSource {
    pub fn repository_root(&self) -> &Path {
        &self.repository_root
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub(crate) fn select_source_path(
    fetched_root: &Path,
    dir: Option<&str>,
) -> Result<MaterializedSource> {
    let fetched_root = fs::canonicalize(fetched_root)
        .into_diagnostic()
        .wrap_err_with(|| {
            format!(
                "The fetched source root does not exist: {}",
                fetched_root.display()
            )
        })?;
    let Some(dir) = dir.filter(|dir| !dir.is_empty()) else {
        return Ok(MaterializedSource {
            path: fetched_root.clone(),
            repository_root: fetched_root,
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
    Ok(MaterializedSource {
        repository_root: fetched_root,
        path: selected,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_source_root_without_dir() {
        let source = tempfile::tempdir().unwrap();

        let selected = select_source_path(source.path(), None).unwrap();
        let root = fs::canonicalize(source.path()).unwrap();

        assert_eq!(selected.repository_root(), root);
        assert_eq!(selected.path(), root);
    }

    #[test]
    fn selects_source_child_from_dir() {
        let source = tempfile::tempdir().unwrap();
        let child = source.path().join("profiles").join("rails");
        fs::create_dir_all(&child).unwrap();

        let selected = select_source_path(source.path(), Some("profiles/rails")).unwrap();

        assert_eq!(
            selected.repository_root(),
            fs::canonicalize(source.path()).unwrap()
        );
        assert_eq!(selected.path(), fs::canonicalize(child).unwrap());
    }

    #[test]
    fn selects_file_input() {
        let source = tempfile::tempdir().unwrap();
        let file = source.path().join("module.nix");
        fs::write(&file, "{}").unwrap();

        for dir in [None, Some("")] {
            let selected = select_source_path(&file, dir).unwrap();
            assert_eq!(selected.path(), fs::canonicalize(&file).unwrap());
        }
    }

    #[test]
    fn rejects_parent_directory_in_dir() {
        let source = tempfile::tempdir().unwrap();

        let error = select_source_path(source.path(), Some("../outside")).unwrap_err();

        assert_eq!(
            error.to_string(),
            "The requested dir '../outside' escapes the source root"
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_outside_source_root() {
        let source = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), source.path().join("linked")).unwrap();

        let error = select_source_path(source.path(), Some("linked")).unwrap_err();

        assert_eq!(
            error.to_string(),
            "The requested dir 'linked' escapes the source root"
        );
    }
}
