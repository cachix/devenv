//! `devenv init`: scaffold devenv.nix / devenv.yaml / .gitignore in a directory.

use std::fs;
use std::io::Write as _;
use std::path::Path;

use console::style;
use devenv_activity::{ActivityLevel, message};
use include_dir::{Dir, File, include_dir};
use miette::{IntoDiagnostic, Result, WrapErr, miette};
use similar::{ChangeTag, TextDiff};

use crate::console as devenv_console;
use crate::tasks::VerbosityLevel;

const PROJECT_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/init");

#[derive(Clone, Copy)]
enum OnExists {
    /// Prompt the user for a diff-based overwrite confirmation.
    Confirm,
    /// Append to the file (with a leading newline). Used for `.gitignore`
    /// so we don't clobber existing ignore entries.
    Append,
}

struct Template {
    /// Name in the bundled `init/` directory. `gitignore` lives without
    /// the leading dot because `include_dir` skips dotfiles.
    source: &'static str,
    /// Filename written to the user's project.
    target: &'static str,
    on_exists: OnExists,
}

const TEMPLATES: &[Template] = &[
    Template {
        source: "devenv.nix",
        target: "devenv.nix",
        on_exists: OnExists::Confirm,
    },
    Template {
        source: "devenv.yaml",
        target: "devenv.yaml",
        on_exists: OnExists::Confirm,
    },
    Template {
        source: "gitignore",
        target: ".gitignore",
        on_exists: OnExists::Append,
    },
];

pub fn run(target: Option<&Path>, verbosity: VerbosityLevel) -> Result<()> {
    let _console = devenv_console::install(verbosity);

    let target = match target {
        Some(p) => p.to_path_buf(),
        None => std::env::current_dir()
            .into_diagnostic()
            .wrap_err("Failed to get current directory")?,
    };

    fs::create_dir_all(&target)
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to create {}", target.display()))?;

    for tmpl in TEMPLATES {
        let file = PROJECT_DIR.get_file(tmpl.source).ok_or_else(|| {
            miette!(
                "Bundled template {} is missing from the binary",
                tmpl.source
            )
        })?;
        let dest = target.join(tmpl.target);
        write_template(file, &dest, tmpl.on_exists)?;
    }

    Ok(())
}

fn write_template(file: &File<'_>, dest: &Path, on_exists: OnExists) -> Result<()> {
    let display_name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| dest.display().to_string());

    if !dest.exists() {
        message(ActivityLevel::Info, format!("Creating {display_name}"));
        return fs::write(dest, file.contents())
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to write {}", dest.display()));
    }

    match on_exists {
        OnExists::Append => {
            message(ActivityLevel::Info, format!("Appending to {display_name}"));
            let mut handle = fs::OpenOptions::new()
                .append(true)
                .open(dest)
                .into_diagnostic()
                .wrap_err_with(|| format!("Failed to open {}", dest.display()))?;
            handle
                .write_all(b"\n")
                .and_then(|_| handle.write_all(file.contents()))
                .into_diagnostic()
                .wrap_err_with(|| format!("Failed to append to {}", dest.display()))
        }
        OnExists::Confirm => {
            let contents = file
                .contents_utf8()
                .ok_or_else(|| miette!("Bundled template {} is not valid UTF-8", dest.display()))?;
            confirm_overwrite(dest, contents)
        }
    }
}

/// Prompt with a diff. Caller guarantees `dest` exists.
fn confirm_overwrite(dest: &Path, contents: &str) -> Result<()> {
    let before = fs::read_to_string(dest)
        .into_diagnostic()
        .wrap_err_with(|| format!("Failed to read {}", dest.display()))?;

    if before == contents {
        return Ok(());
    }

    eprintln!("\nChanges that will be made to {}:", dest.display());
    for change in TextDiff::from_lines(before.as_str(), contents).iter_all_changes() {
        match change.tag() {
            ChangeTag::Delete => eprint!("{}{change}", style("-").red()),
            ChangeTag::Insert => eprint!("{}{change}", style("+").green()),
            ChangeTag::Equal => eprint!(" {change}"),
        }
    }

    let confirmed = dialoguer::Confirm::new()
        .with_prompt(format!("{} already exists. Overwrite it?", dest.display()))
        .interact()
        .into_diagnostic()?;

    if confirmed {
        fs::write(dest, contents)
            .into_diagnostic()
            .wrap_err_with(|| format!("Failed to write {}", dest.display()))?;
    }
    Ok(())
}
