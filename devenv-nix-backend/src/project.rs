//! Compose project configuration and its lock together, before evaluation.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use devenv_core::config::{Config, Input};
use devenv_core::{NixOptions, NixSettings, StoreSettings};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use nix_bindings_fetchers::FetchersSettings;
use nix_bindings_flake::LockFile;

use crate::anyhow_ext::AnyhowToMiette;

const MAX_IMPORT_PASSES: usize = 100;

/// All caller-owned configuration sources. Reloading always applies the same
/// inputs and overrides, including inputs discovered in a later pass.
pub struct ProjectConfig {
    pub root: PathBuf,
    pub source_dir: Option<PathBuf>,
    pub input_overrides: Vec<String>,
    /// Programmatic inputs, such as the test runner's local devenv modules.
    pub extra_inputs: BTreeMap<String, Input>,
}

impl ProjectConfig {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            source_dir: None,
            input_overrides: Vec::new(),
            extra_inputs: BTreeMap::new(),
        }
    }

    fn load(&self, sources: &BTreeMap<String, PathBuf>, defer_unknown: bool) -> Result<Config> {
        let mut config = Config::load_from_with_source_and_input_sources(
            &self.root,
            self.source_dir.as_deref(),
            sources,
        )?;
        if !self.input_overrides.len().is_multiple_of(2) {
            bail!("Input overrides must contain name/URL pairs");
        }
        for pair in self.input_overrides.chunks_exact(2) {
            let name = &pair[0];
            if defer_unknown
                && !config.inputs.contains_key(name)
                && name != "nixpkgs"
                && name != "devenv"
            {
                continue;
            }
            config
                .override_input_url(name, &pair[1])
                .wrap_err_with(|| {
                    format!("Failed to override input {name} with URL {}", pair[1])
                })?;
        }
        config.inputs.extend(self.extra_inputs.clone());
        Ok(config)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub enum UpdatePolicy<'a> {
    #[default]
    Preserve,
    All,
    Input(&'a str),
}

/// A consistent configuration and lock. Resolution does not write to disk.
pub struct ResolvedProject {
    pub config: Config,
    pub sources: BTreeMap<String, PathBuf>,
    lock: Option<LockFile>,
    root: PathBuf,
}

impl ResolvedProject {
    pub fn write_lock(&self) -> Result<()> {
        if let Some(lock) = &self.lock {
            crate::write_lock_file(lock, &self.root.join("devenv.lock"))
                .to_miette()
                .wrap_err("Failed to write composed lock file")?;
        }
        Ok(())
    }
}

/// Discover imported YAML using the revisions selected by `update`. Retain
/// existing pins during incomplete passes and freeze each requested update
/// once selected. Only the final candidate is returned for writing.
pub fn resolve_project(
    project: &ProjectConfig,
    nix_options: NixOptions,
    update: UpdatePolicy<'_>,
) -> Result<ResolvedProject> {
    let sources = BTreeMap::new();
    let mut config = project.load(&sources, true)?;
    if config.input_import_names().is_empty() && matches!(update, UpdatePolicy::Preserve) {
        return Ok(ResolvedProject {
            config: project.load(&sources, false)?,
            sources,
            lock: None,
            root: project.root.clone(),
        });
    }

    let mut settings = NixSettings::resolve(nix_options, &config);
    settings.refresh_fetchers = !matches!(update, UpdatePolicy::Preserve);
    let _gc_registration = crate::backend::init_nix(&settings, &StoreSettings::default())?;
    let store = crate::backend::open_store(&StoreSettings::default())?;
    let (flake_settings, fetchers_settings) = crate::backend::build_settings()?;
    let mut eval_state = crate::lock::build_eval_state(
        &store,
        &project.root,
        &flake_settings,
        settings.refresh_fetchers,
    )?;
    let mut baseline = crate::load_lock_file(&fetchers_settings, &project.root.join("devenv.lock"))
        .to_miette()
        .wrap_err("Failed to load lock file while composing project")?;
    let mut updated = BTreeSet::new();

    for _ in 0..MAX_IMPORT_PASSES {
        let updates: Vec<String> = match update {
            UpdatePolicy::Preserve => Vec::new(),
            UpdatePolicy::All => config
                .inputs
                .keys()
                .cloned()
                .chain(["nixpkgs".into(), "devenv".into()])
                .filter(|name| !updated.contains(name))
                .collect(),
            UpdatePolicy::Input(name)
                if !updated.contains(name)
                    && (config
                        .inputs
                        .contains_key(name.split('/').next().unwrap_or(name))
                        || name == "nixpkgs"
                        || name == "devenv") =>
            {
                vec![name.into()]
            }
            UpdatePolicy::Input(_) => Vec::new(),
        };
        let candidate = crate::resolve_lock_file(
            &eval_state,
            &fetchers_settings,
            &flake_settings,
            &project.root,
            &config.inputs,
            baseline.as_ref(),
            &updates,
        )
        .to_miette()
        .wrap_err("Failed to resolve project inputs")?;
        updated.extend(updates);
        let next_sources = crate::lock::input_paths_from_lock(
            &mut eval_state,
            &project.root,
            &config.input_import_names(),
            &candidate,
        )?;
        let next = project.load(&next_sources, true)?;
        let stable = next.inputs == config.inputs && next.imports == config.imports;
        if stable {
            if let UpdatePolicy::Input(name) = update
                && !updated.contains(name)
            {
                bail!("Cannot update unknown input '{name}'");
            }
            return Ok(ResolvedProject {
                config: project.load(&next_sources, false)?,
                sources: next_sources,
                lock: Some(candidate),
                root: project.root.clone(),
            });
        }
        baseline = Some(retain_missing_pins(
            &fetchers_settings,
            baseline.as_ref(),
            candidate,
        )?);
        config = next;
    }
    bail!("Remote devenv.yaml imports did not stabilize after {MAX_IMPORT_PASSES} passes")
}

/// Keep root edges absent from an incomplete candidate. Node IDs are local to
/// each graph, so copied nodes must be renamed, including their direct edges.
/// Follows edges remain paths relative to the composed root.
fn retain_missing_pins(
    fetchers: &FetchersSettings,
    previous: Option<&LockFile>,
    candidate: LockFile,
) -> Result<LockFile> {
    let Some(previous) = previous else {
        return Ok(candidate);
    };
    let old: LockGraph =
        serde_json::from_str(&previous.to_string().to_miette()?).into_diagnostic()?;
    let mut next: LockGraph =
        serde_json::from_str(&candidate.to_string().to_miette()?).into_diagnostic()?;
    let old_root = old
        .nodes
        .get(&old.root)
        .ok_or_else(|| miette::miette!("Missing old lock root"))?;
    let next_root = next
        .nodes
        .get(&next.root)
        .ok_or_else(|| miette::miette!("Missing candidate lock root"))?;
    let pending: BTreeMap<_, _> = old_root
        .inputs
        .iter()
        .filter(|(name, _)| !next_root.inputs.contains_key(*name))
        .map(|(name, edge)| (name.clone(), edge.clone()))
        .collect();
    if pending.is_empty() {
        return Ok(candidate);
    }
    let mut renamed = BTreeMap::from([(old.root.clone(), next.root.clone())]);
    for (name, mut edge) in pending {
        copy_edge(&mut edge, &old, &mut next.nodes, &mut renamed)?;
        next.nodes
            .get_mut(&next.root)
            .ok_or_else(|| miette::miette!("Missing candidate lock root"))?
            .inputs
            .insert(name, edge);
    }
    let json = serde_json::to_string(&next).into_diagnostic()?;
    LockFile::parse(fetchers, &json, None).to_miette()
}

#[derive(serde::Serialize, serde::Deserialize)]
struct LockGraph {
    root: String,
    nodes: BTreeMap<String, LockNode>,
    #[serde(flatten)]
    metadata: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
struct LockNode {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    inputs: BTreeMap<String, LockEdge>,
    #[serde(flatten)]
    attributes: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
enum LockEdge {
    Node(String),
    Follows(Vec<String>),
}

fn copy_edge(
    edge: &mut LockEdge,
    old: &LockGraph,
    nodes: &mut BTreeMap<String, LockNode>,
    renamed: &mut BTreeMap<String, String>,
) -> Result<()> {
    let LockEdge::Node(key) = edge else {
        return Ok(());
    };
    let name = if let Some(name) = renamed.get(key) {
        name.clone()
    } else {
        let mut index = nodes.len();
        let name = loop {
            let name = format!("retained_{index}");
            if !nodes.contains_key(&name) {
                break name;
            }
            index += 1;
        };
        renamed.insert(key.clone(), name.clone());
        nodes.insert(name.clone(), LockNode::default());
        let mut node = old
            .nodes
            .get(key)
            .cloned()
            .ok_or_else(|| miette::miette!("Missing retained lock node '{key}'"))?;
        for edge in node.inputs.values_mut() {
            copy_edge(edge, old, nodes, renamed)?;
        }
        nodes.insert(name.clone(), node);
        name
    };
    *key = name;
    Ok(())
}
