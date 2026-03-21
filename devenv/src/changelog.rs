use devenv_cache_core::compute_string_hash;
use devenv_core::{DevenvPaths, NixBackend, Options};
use miette::{IntoDiagnostic, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use termimad::MadSkin;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangelogEntry {
    pub date: String,
    pub title: String,
    pub description: String,
}

impl ChangelogEntry {
    /// Generate a unique hash for this changelog entry based on date and title
    pub fn hash(&self) -> String {
        let content = format!("{}:{}", self.date, self.title);
        compute_string_hash(&content)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ChangelogCache {
    /// Hashes of changelog entries that have been shown
    shown_hashes: HashSet<String>,
}

pub struct Changelog<'a> {
    nix: &'a dyn NixBackend,
    dot_gc: PathBuf,
    cache_file: PathBuf,
}

impl<'a> Changelog<'a> {
    pub fn new(nix: &'a dyn NixBackend, paths: &DevenvPaths) -> Self {
        Self {
            nix,
            dot_gc: paths.dot_gc.clone(),
            cache_file: paths.dotfile.join("changelog-cache.json"),
        }
    }

    pub async fn show_new(&self) -> Result<Option<String>> {
        // Load all current changelogs
        let all_changelogs = match self.load_changelogs().await {
            Ok(changelogs) => changelogs,
            Err(_) => {
                // Changelog module might not exist in older devenv versions
                tracing::warn!(
                    "Changelog not available. Update devenv modules for changelog support."
                );
                return Ok(None);
            }
        };

        // Load cache of seen entries
        let mut cache = load_cache(&self.cache_file)?;

        // Filter to unseen entries
        let new_changelogs = self.filter_unseen_changelogs(all_changelogs, &cache);

        if new_changelogs.is_empty() {
            return Ok(None);
        }

        // Render changelogs to string
        let output = Self::render_changelogs(&new_changelogs);

        // Update cache with newly shown hashes
        for entry in &new_changelogs {
            cache.shown_hashes.insert(entry.hash());
        }

        // Save updated cache
        save_cache(&self.cache_file, &cache)?;

        Ok(Some(output))
    }

    pub async fn show_all(&self) -> Result<Option<String>> {
        let all_changelogs = match self.load_changelogs().await {
            Ok(changelogs) => changelogs,
            Err(_) => {
                tracing::warn!(
                    "Changelog not available. Update devenv modules for changelog support."
                );
                return Ok(None);
            }
        };
        if all_changelogs.is_empty() {
            return Ok(None);
        }
        Ok(Some(Self::render_changelogs(&all_changelogs)))
    }

    async fn load_changelogs(&self) -> Result<Vec<ChangelogEntry>> {
        let changelog_json_file = {
            let gc_root = self.dot_gc.join("changelog-json");
            let options = Options {
                bail_on_error: false,
                ..Default::default()
            };
            self.nix
                .build(
                    &["devenv.config.changelog.json"],
                    Some(options),
                    Some(&gc_root),
                )
                .await?
        };

        let changelog_path = changelog_json_file
            .first()
            .ok_or_else(|| miette::miette!("No changelog output produced by build"))?;

        let changelog_json = tokio::fs::read_to_string(changelog_path)
            .await
            .into_diagnostic()?;

        let changelogs: Vec<ChangelogEntry> =
            serde_json::from_str(&changelog_json).into_diagnostic()?;

        Ok(changelogs)
    }

    fn filter_unseen_changelogs(
        &self,
        changelogs: Vec<ChangelogEntry>,
        cache: &ChangelogCache,
    ) -> Vec<ChangelogEntry> {
        let mut unseen: Vec<_> = changelogs
            .into_iter()
            .filter(|entry| !cache.shown_hashes.contains(&entry.hash()))
            .collect();

        // Sort by date (oldest first)
        unseen.sort_by(|a, b| a.date.cmp(&b.date));

        unseen
    }

    /// Render changelogs with markdown rendering to a string
    fn render_changelogs(changelogs: &[ChangelogEntry]) -> String {
        use std::fmt::Write;

        let mut output = String::new();
        let skin = MadSkin::default();

        writeln!(output, "\n📋 changelog\n").unwrap();

        for entry in changelogs {
            // Format: date: **title**
            let header = format!("{}: **{}**", entry.date, entry.title);
            writeln!(output, "{}", skin.inline(&header)).unwrap();
            writeln!(output).unwrap();

            // Render markdown description with indentation
            for line in entry.description.lines() {
                if line.trim().is_empty() {
                    writeln!(output).unwrap();
                } else {
                    writeln!(output, "  {}", skin.inline(line)).unwrap();
                }
            }
            writeln!(output).unwrap();
        }

        output
    }
}

fn load_cache(cache_file: &Path) -> Result<ChangelogCache> {
    if !cache_file.exists() {
        return Ok(ChangelogCache::default());
    }

    let content = std::fs::read_to_string(cache_file).into_diagnostic()?;

    let cache: ChangelogCache = serde_json::from_str(&content).into_diagnostic()?;

    Ok(cache)
}

fn save_cache(cache_file: &Path, cache: &ChangelogCache) -> Result<()> {
    if let Some(parent) = cache_file.parent() {
        std::fs::create_dir_all(parent).into_diagnostic()?;
    }

    let content = serde_json::to_string_pretty(cache).into_diagnostic()?;

    std::fs::write(cache_file, content).into_diagnostic()?;

    Ok(())
}
