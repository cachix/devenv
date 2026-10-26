//! Compose project configuration and its lock together, before evaluation.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use devenv_core::config::{Config, ConfigLoader, ConfigSource, Input};
use devenv_core::{NixOptions, NixSettings, StoreSettings};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use nix_bindings_fetchers::FetchersSettings;
use nix_bindings_flake::LockFile;

use crate::anyhow_ext::AnyhowToMiette;
use crate::source::MaterializedSource;

const MAX_IMPORT_PASSES: usize = 100;

/// All caller-owned configuration sources. Reloading always applies the same
/// inputs and overrides, including inputs discovered in a later pass.
pub struct ProjectConfig {
    pub root: PathBuf,
    pub source: Option<ProjectSource>,
    pub input_overrides: Vec<String>,
    /// Programmatic inputs, such as the test runner's local devenv modules.
    pub extra_inputs: BTreeMap<String, Input>,
}

/// The source selected by `--from`. Local paths stay live; input references
/// are locked and materialized before composing their YAML configuration.
pub enum ProjectSource {
    Local(PathBuf),
    Input(Input),
}

impl ProjectSource {
    /// Interpret a `--from` reference relative to the invocation directory.
    /// `path:` references remain live instead of being copied into the store.
    pub fn from_reference(reference: &str, base_dir: &Path) -> Self {
        match reference.strip_prefix("path:") {
            Some(path) => {
                let path = devenv_core::paths::resolve_against(Path::new(path), base_dir);
                Self::Local(path.canonicalize().unwrap_or(path))
            }
            None => Self::Input(Input {
                url: Some(reference.into()),
                flake: false,
                ..Default::default()
            }),
        }
    }
}

/// Discovery can encounter overrides for inputs declared by a later import.
/// Once imports stabilize, every override must refer to a known input.
#[derive(Clone, Copy)]
enum LoadPhase {
    Discover,
    Validate,
}

impl ProjectConfig {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            source: None,
            input_overrides: Vec::new(),
            extra_inputs: BTreeMap::new(),
        }
    }

    fn load(
        &self,
        sources: &BTreeMap<String, PathBuf>,
        source: Option<&MaterializedSource>,
        phase: LoadPhase,
    ) -> Result<Config> {
        let source = match (source, &self.source) {
            (Some(source), _) => Some(ConfigSource::Fetched {
                directory: source.path(),
                repository_root: source.repository_root(),
            }),
            (None, Some(ProjectSource::Local(directory))) => Some(ConfigSource::Local(directory)),
            _ => None,
        };
        let mut loader = ConfigLoader::new(&self.root).with_input_sources(sources);
        if let Some(source) = source {
            loader = loader.with_source(source);
        }
        let mut config = loader.load()?;
        if !self.input_overrides.len().is_multiple_of(2) {
            bail!("Input overrides must contain name/URL pairs");
        }
        for pair in self.input_overrides.chunks_exact(2) {
            let name = &pair[0];
            if matches!(phase, LoadPhase::Discover)
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
        // Preserve the source reference in the final lock, rather than its
        // materialized store path. Apply overrides before adding this input.
        if let Some(ProjectSource::Input(input)) = &self.source {
            config.inputs.insert("from".into(), input.clone());
        }
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
    // Nix resolves relative flake references using an absolute source path.
    // In particular, passing "./devenv.nix" makes .?dir=... resolve at /.
    let root = project
        .root
        .canonicalize()
        .into_diagnostic()
        .wrap_err_with(|| {
            format!(
                "Failed to canonicalize project root: {}",
                project.root.display()
            )
        })?;
    let sources = BTreeMap::new();
    let mut config = project.load(&sources, None, LoadPhase::Discover)?;
    if !matches!(project.source, Some(ProjectSource::Input(_)))
        && config.input_import_names().is_empty()
        && matches!(update, UpdatePolicy::Preserve)
    {
        return Ok(ResolvedProject {
            config: project.load(&sources, None, LoadPhase::Validate)?,
            sources,
            lock: None,
            root: root.clone(),
        });
    }

    let mut settings = NixSettings::resolve(nix_options, &config);
    settings.refresh_fetchers = !matches!(update, UpdatePolicy::Preserve);
    let _gc_registration = crate::backend::init_nix(&settings, &StoreSettings::default())?;
    let store = crate::backend::open_store(&StoreSettings::default())?;
    let (flake_settings, fetchers_settings) = crate::backend::build_settings()?;
    let mut baseline = crate::load_lock_file(&fetchers_settings, &root.join("devenv.lock"))
        .to_miette()
        .wrap_err("Failed to load lock file while composing project")?;
    let mut updated = BTreeSet::new();
    let source = if let Some(ProjectSource::Input(input)) = &project.source {
        let refresh_source = matches!(update, UpdatePolicy::All | UpdatePolicy::Input("from"));
        // An unrelated input update must not refresh the source's fetcher cache.
        let mut source_eval =
            crate::lock::build_eval_state(&store, &root, &flake_settings, refresh_source)?;
        let source_lock = crate::lock_source_input(
            &source_eval,
            &fetchers_settings,
            &flake_settings,
            &root,
            baseline.as_ref(),
            input,
            refresh_source,
        )
        .to_miette()
        .wrap_err("Failed to lock the source input")?;
        let mut source_paths = crate::lock::input_sources_from_lock(
            &mut source_eval,
            &root,
            &BTreeSet::from(["from".into()]),
            &source_lock,
        )?;
        let source = source_paths
            .remove("from")
            .ok_or_else(|| miette::miette!("Resolved source input 'from' is missing"))?;
        if !source.path().is_dir() {
            bail!(
                "The selected source path is not a directory: {}",
                source.path().display()
            );
        }
        baseline = Some(retain_missing_pins(
            &fetchers_settings,
            baseline.as_ref(),
            source_lock,
        )?);
        // Freeze the selected source revision across subsequent import passes.
        if refresh_source {
            updated.insert("from".into());
        }
        config = project.load(&sources, Some(&source), LoadPhase::Discover)?;
        Some(source)
    } else {
        None
    };
    let mut eval_state =
        crate::lock::build_eval_state(&store, &root, &flake_settings, settings.refresh_fetchers)?;

    for _ in 0..MAX_IMPORT_PASSES {
        let (discovery_inputs, pending_follows) = discovery_inputs(&config.inputs);
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
        }
        .into_iter()
        // A deferred alias has no node to update until its target is known.
        .filter(|name: &String| {
            !pending_follows.contains_key(name.split('/').next().unwrap_or(name))
        })
        .collect();
        let candidate = crate::resolve_lock_file(
            &eval_state,
            &fetchers_settings,
            &flake_settings,
            &root,
            &discovery_inputs,
            baseline.as_ref(),
            &updates,
        )
        .to_miette()
        .wrap_err("Failed to resolve project inputs")?;
        updated.extend(updates);
        let import_names = config
            .input_import_names()
            .into_iter()
            .filter(|name| !pending_follows.contains_key(name))
            .collect();
        let next_sources =
            crate::lock::input_paths_from_lock(&mut eval_state, &root, &import_names, &candidate)?;
        let next = project.load(&next_sources, source.as_ref(), LoadPhase::Discover)?;
        let stable = next.inputs == config.inputs && next.imports == config.imports;
        if stable {
            if !pending_follows.is_empty() {
                let unresolved = pending_follows
                    .iter()
                    .map(|(name, target)| format!("'{name}' follows missing input '{target}'"))
                    .collect::<Vec<_>>()
                    .join(", ");
                bail!("Unresolved follows after composing project: {unresolved}");
            }
            if let UpdatePolicy::Input(name) = update
                && !updated.contains(name)
            {
                bail!("Cannot update unknown input '{name}'");
            }
            return Ok(ResolvedProject {
                config: project.load(&next_sources, source.as_ref(), LoadPhase::Validate)?,
                sources: next_sources,
                lock: Some(candidate),
                root: root.clone(),
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

/// Ignore follows edges whose root target has not been discovered yet. Keep
/// deferred root names as empty declarations so the default nixpkgs/devenv
/// inputs are not fetched in their place. The complete config is never changed,
/// and resolution cannot succeed while any follows edges remain deferred.
fn discovery_inputs(
    inputs: &BTreeMap<String, Input>,
) -> (BTreeMap<String, Input>, BTreeMap<String, String>) {
    let mut available: BTreeSet<_> = inputs
        .iter()
        .filter(|(_, input)| input.url.is_some() || input.follows.is_some())
        .map(|(name, _)| name.as_str())
        .chain(
            ["nixpkgs", "devenv"]
                .into_iter()
                .filter(|name| !inputs.contains_key(*name)),
        )
        .collect();
    // Propagate missing targets through aliases, including aliases declared
    // before their targets in the map. Leave cycles to Nix's validation.
    loop {
        let missing: Vec<_> = available
            .iter()
            .copied()
            .filter(|name| {
                inputs
                    .get(*name)
                    .and_then(|input| input.follows.as_deref())
                    .is_some_and(|target| {
                        !target.is_empty() && !available.contains(target.split('/').next().unwrap())
                    })
            })
            .collect();
        if missing.is_empty() {
            break;
        }
        for name in missing {
            available.remove(name);
        }
    }
    let mut discovery = inputs.clone();
    let mut pending = BTreeMap::new();
    for (name, input) in &mut discovery {
        if let Some(target) = &input.follows
            && !available.contains(name.as_str())
        {
            pending.insert(name.clone(), target.clone());
            *input = Input::default();
            continue;
        }
        input.inputs.retain(|nested_name, nested| {
            if let Some(target) = &nested.follows
                && !target.is_empty()
                && !available.contains(target.split('/').next().unwrap())
            {
                pending.insert(format!("{name}/{nested_name}"), target.clone());
                false
            } else {
                true
            }
        });
    }
    (discovery, pending)
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
