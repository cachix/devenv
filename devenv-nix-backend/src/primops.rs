//! Devenv primop registrations for the C-Nix evaluator.
//!
//! A registration owns both the metadata needed to bind a primop and the
//! handler that executes it. [`NixCBackend`](crate::NixCBackend) only asks the
//! registry to build an attribute set; it does not need to know which primops
//! are installed or what state they use.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cstr::cstr;
use devenv_core::dotenv::load_dotenv_tracked;
use devenv_core::ports::PortAllocator;
use devenv_eval_cache::{EvalContext, EvalInputTracker, EvalResourceRegistry};
use miette::{Result, WrapErr, bail};
use nix_bindings_expr::eval_state::EvalState;
use nix_bindings_expr::primop::{PrimOp, PrimOpMeta};
use nix_bindings_expr::value::Value;

use crate::anyhow_ext::AnyhowToMiette;

/// A primop that can bind itself into an [`EvalState`].
pub trait PrimopRegistration: Send + Sync {
    /// Attribute name used in the injected `primops` set.
    fn name(&self) -> &'static str;

    /// Whether this primop should be present for the current evaluation.
    fn is_enabled(&self) -> bool {
        true
    }

    /// Stable state that affects evaluation output and therefore cache keys.
    fn cache_key_fragment(&self) -> String {
        format!("enabled={}", self.is_enabled())
    }

    /// Build the primop value and return the attribute name it is bound under.
    fn register(&self, eval_state: &mut EvalState) -> Result<(String, Value)>;
}

/// The primops made available to devenv's bootstrap expression.
#[derive(Default)]
pub struct PrimopRegistry {
    registrations: Vec<Arc<dyn PrimopRegistration>>,
}

impl PrimopRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add<T>(&mut self, registration: T)
    where
        T: PrimopRegistration + 'static,
    {
        self.registrations.push(Arc::new(registration));
    }

    pub fn register_all(&self, eval_state: &mut EvalState) -> Result<Value> {
        let mut names = HashSet::with_capacity(self.registrations.len());
        for registration in &self.registrations {
            if !names.insert(registration.name()) {
                bail!("Duplicate primop registration: {}", registration.name());
            }
        }
        let values = self
            .registrations
            .iter()
            .filter(|registration| registration.is_enabled())
            .map(|registration| registration.register(eval_state))
            .collect::<Result<Vec<_>>>()?;

        eval_state
            .new_value_attrs(values)
            .to_miette()
            .wrap_err("Failed to create primops attrset")
    }

    /// Deterministic fingerprint of every installed primop's cache-relevant state.
    pub fn cache_key_fragment(&self) -> String {
        let mut fragments = self
            .registrations
            .iter()
            .map(|registration| {
                format!(
                    "{}:{}",
                    registration.name(),
                    registration.cache_key_fragment()
                )
            })
            .collect::<Vec<_>>();
        fragments.sort();
        fragments.join(",")
    }
}

/// Composition root for evaluation extensions.
///
/// Extensions install primops and replayable resources together, while all
/// input-producing handlers share the same native file/env tracker.
pub struct NixEvalSetup {
    primops: PrimopRegistry,
    resources: EvalResourceRegistry,
    inputs: Arc<EvalInputTracker>,
}

impl NixEvalSetup {
    pub fn new() -> Self {
        Self {
            primops: PrimopRegistry::new(),
            resources: EvalResourceRegistry::new(),
            inputs: EvalInputTracker::new(),
        }
    }

    pub fn install<P: NixEvalPlugin>(&mut self, plugin: P) -> &mut Self {
        plugin.install(self);
        self
    }

    pub fn add_primop<P: PrimopRegistration + 'static>(&mut self, primop: P) {
        self.primops.add(primop);
    }

    pub fn add_resource<R: devenv_core::ReplayableResource + 'static>(&mut self, resource: Arc<R>) {
        self.resources.register(resource);
    }

    pub fn inputs(&self) -> Arc<EvalInputTracker> {
        self.inputs.clone()
    }

    pub fn finish(self) -> (PrimopRegistry, EvalContext) {
        (
            self.primops,
            EvalContext::new(Arc::new(self.resources), self.inputs),
        )
    }
}

impl Default for NixEvalSetup {
    fn default() -> Self {
        Self::new()
    }
}

pub trait NixEvalPlugin {
    fn install(self, setup: &mut NixEvalSetup);
}

pub struct DotenvPlugin {
    root: PathBuf,
}

impl DotenvPlugin {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl NixEvalPlugin for DotenvPlugin {
    fn install(self, setup: &mut NixEvalSetup) {
        setup.add_primop(LoadDotenvPrimop::new(self.root, setup.inputs.clone()));
    }
}

pub struct PortAllocationPlugin {
    allocator: Arc<PortAllocator>,
}

impl PortAllocationPlugin {
    pub fn new(allocator: Arc<PortAllocator>) -> Self {
        Self { allocator }
    }
}

impl NixEvalPlugin for PortAllocationPlugin {
    fn install(self, setup: &mut NixEvalSetup) {
        setup.add_resource(self.allocator.clone());
        setup.add_primop(AllocatePortPrimop::new(self.allocator));
    }
}

/// Registration for `loadDotenv`.
pub struct LoadDotenvPrimop {
    root: PathBuf,
    tracker: Arc<EvalInputTracker>,
}

impl LoadDotenvPrimop {
    pub fn new(root: PathBuf, tracker: Arc<EvalInputTracker>) -> Self {
        Self { root, tracker }
    }
}

impl PrimopRegistration for LoadDotenvPrimop {
    fn name(&self) -> &'static str {
        "loadDotenv"
    }

    fn register(&self, eval_state: &mut EvalState) -> Result<(String, Value)> {
        let root = self.root.clone();
        let tracker = self.tracker.clone();
        let primop = PrimOp::new(
            eval_state,
            PrimOpMeta {
                name: cstr!("loadDotenv"),
                doc: cstr!("Load dotenv files without parsing them in Nix"),
                args: [cstr!("filenames"), cstr!("substitution")],
            },
            Box::new(move |es, [filenames, substitution]| {
                let filename_values = es.require_list_strict::<Vec<_>>(filenames)?;
                let mut paths = Vec::with_capacity(filename_values.len());
                for filename in filename_values {
                    let filename = es.require_string(&filename)?;
                    let path = Path::new(&filename);
                    paths.push(if path.is_absolute() {
                        path.to_path_buf()
                    } else {
                        root.join(path)
                    });
                }
                let substitution = es.require_bool(substitution)?;
                let variables = load_dotenv_tracked(&paths, substitution, tracker.as_ref())
                    .map_err(|error| anyhow::anyhow!("{error:?}"))?;
                let mut attrs = Vec::with_capacity(variables.len());
                for (name, value) in variables {
                    attrs.push((name, es.new_value_str(&value)?));
                }
                es.new_value_attrs(attrs)
            }),
        )
        .to_miette()
        .wrap_err("Failed to create loadDotenv primop")?;
        let value = eval_state
            .new_value_primop(primop)
            .to_miette()
            .wrap_err("Failed to create loadDotenv primop value")?;
        Ok(("loadDotenv".to_string(), value))
    }
}

/// Registration for `allocatePort`.
pub struct AllocatePortPrimop {
    allocator: Arc<PortAllocator>,
}

impl AllocatePortPrimop {
    pub fn new(allocator: Arc<PortAllocator>) -> Self {
        Self { allocator }
    }
}

impl PrimopRegistration for AllocatePortPrimop {
    fn name(&self) -> &'static str {
        "allocatePort"
    }

    fn is_enabled(&self) -> bool {
        self.allocator.is_enabled()
    }

    fn cache_key_fragment(&self) -> String {
        format!(
            "enabled={}:strict={}",
            self.allocator.is_enabled(),
            self.allocator.is_strict()
        )
    }

    fn register(&self, eval_state: &mut EvalState) -> Result<(String, Value)> {
        let allocator = self.allocator.clone();
        let primop = PrimOp::new(
            eval_state,
            PrimOpMeta {
                name: cstr!("allocatePort"),
                doc: cstr!("Allocate a free port starting from base"),
                args: [cstr!("processName"), cstr!("portName"), cstr!("basePort")],
            },
            Box::new(move |es, [process_name, port_name, base_port]| {
                let process = es.require_string(process_name)?;
                let port_name = es.require_string(port_name)?;
                let base_raw = es.require_int(base_port)?;
                let base = u16::try_from(base_raw).map_err(|_| {
                    anyhow::anyhow!("basePort must be between 0 and 65535, got {base_raw}")
                })?;
                let allocated = allocator
                    .allocate(&process, &port_name, base)
                    .map_err(|error| anyhow::anyhow!("{error}"))?;
                es.new_value_int(i64::from(allocated))
            }),
        )
        .to_miette()
        .wrap_err("Failed to create allocatePort primop")?;
        let value = eval_state
            .new_value_primop(primop)
            .to_miette()
            .wrap_err("Failed to create allocatePort primop value")?;
        Ok(("allocatePort".to_string(), value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use devenv_core::ReplayableResource;

    #[test]
    fn port_plugin_installs_primop_resource_and_cache_state() {
        let allocator = Arc::new(PortAllocator::new());
        allocator.set_enabled(true);
        allocator.set_strict(true);
        let mut setup = NixEvalSetup::new();

        setup.install(PortAllocationPlugin::new(allocator));

        assert!(setup.resources.handles(PortAllocator::TYPE_ID));
        assert!(
            setup
                .primops
                .cache_key_fragment()
                .contains("allocatePort:enabled=true:strict=true")
        );
        assert!(setup.resources.snapshot_all().unwrap().is_empty());
    }

    #[test]
    fn primop_cache_fingerprint_is_registration_order_independent() {
        let first_allocator = Arc::new(PortAllocator::new());
        let mut first = PrimopRegistry::new();
        first.add(AllocatePortPrimop::new(first_allocator));
        first.add(LoadDotenvPrimop::new(
            PathBuf::from("/project"),
            EvalInputTracker::new(),
        ));

        let second_allocator = Arc::new(PortAllocator::new());
        let mut second = PrimopRegistry::new();
        second.add(LoadDotenvPrimop::new(
            PathBuf::from("/project"),
            EvalInputTracker::new(),
        ));
        second.add(AllocatePortPrimop::new(second_allocator));

        assert_eq!(first.cache_key_fragment(), second.cache_key_fragment());
    }
}
