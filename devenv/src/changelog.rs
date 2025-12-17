use blake3;
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
        let hash = blake3::hash(content.as_bytes());
        hash.to_hex().to_string()
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

    pub async fn show_new(&self) -> Result<()> {
        // Load all current changelogs
        let all_changelogs = match self.load_changelogs().await {
            Ok(changelogs) => changelogs,
            Err(_) => {
                // Changelog module might not exist in older devenv versions
                tracing::warn!(
                    "Changelog not available. Update devenv modules for changelog support."
                );
                return Ok(());
            }
        };

        // Load cache of seen entries
        let mut cache = load_cache(&self.cache_file)?;

        // Filter to unseen entries
        let new_changelogs = self.filter_unseen_changelogs(all_changelogs, &cache);

        if !new_changelogs.is_empty() {
            // Display new changelogs
            self.display_changelogs(&new_changelogs)?;

            // Update cache with newly shown hashes
            for entry in &new_changelogs {
                cache.shown_hashes.insert(entry.hash());
            }

            // Save updated cache
            save_cache(&self.cache_file, &cache)?;
        }

        Ok(())
    }

    pub async fn show_all(&self) -> Result<()> {
        let all_changelogs = match self.load_changelogs().await {
            Ok(changelogs) => changelogs,
            Err(_) => {
                tracing::warn!(
                    "Changelog not available. Update devenv modules for changelog support."
                );
                return Ok(());
            }
        };
        self.display_changelogs(&all_changelogs)?;
        Ok(())
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

        let changelog_json = tokio::fs::read_to_string(&changelog_json_file[0])
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

    /// Display changelogs with markdown rendering
    fn display_changelogs(&self, changelogs: &[ChangelogEntry]) -> Result<()> {
        if changelogs.is_empty() {
            return Ok(());
        }

        println!("\n📋 changelog\n");

        let skin = MadSkin::default();

        for entry in changelogs {
            // Format: date: **title**
            let header = format!("{}: **{}**", entry.date, entry.title);
            println!("{}", skin.inline(&header));
            println!();

            // Render markdown description with indentation
            let lines = entry.description.lines();
            for line in lines {
                if line.trim().is_empty() {
                    println!();
                } else {
                    println!("  {}", skin.inline(line));
                }
            }
            println!();
        }

        Ok(())
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
