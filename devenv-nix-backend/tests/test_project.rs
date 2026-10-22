#![cfg(feature = "test-nix-store")]

use std::fs;
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use devenv_core::NixOptions;
use devenv_nix_backend::project::{ProjectConfig, ResolvedProject, UpdatePolicy, resolve_project};
use devenv_nix_backend_macros::nix_test;
use serde_json::Value;
use tempfile::TempDir;

mod common;

fn commit(dir: &Path) {
    for args in [
        ["add", "."].as_slice(),
        ["commit", "-qm", "update configuration"].as_slice(),
    ] {
        assert!(
            Command::new("git")
                .args([
                    "-c",
                    "user.email=test@devenv",
                    "-c",
                    "user.name=devenv-test",
                    "-c",
                    "commit.gpgsign=false",
                    "-C"
                ])
                .arg(dir)
                .args(args)
                .status()
                .expect("run git")
                .success()
        );
    }
}

// file:// inputs intentionally remain live and omit revisions from the lock.
// Serve only this fixture's repositories over loopback to exercise real pins.
struct GitDaemon {
    child: Child,
    port: u16,
}

impl GitDaemon {
    fn new(root: &Path) -> Self {
        let socket = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        drop(socket);
        let child = Command::new("git")
            .args([
                "daemon",
                "--reuseaddr",
                "--export-all",
                "--listen=127.0.0.1",
            ])
            .arg(format!("--port={port}"))
            .arg(format!("--base-path={}", root.display()))
            .arg(root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut daemon = Self { child, port };
        for _ in 0..200 {
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                return daemon;
            }
            assert!(
                daemon.child.try_wait().unwrap().is_none(),
                "git daemon exited before becoming ready"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("git daemon did not become ready");
    }

    fn url(&self, repo: &Path) -> String {
        format!(
            "git://127.0.0.1:{}/{}",
            self.port,
            repo.file_name().unwrap().to_str().unwrap()
        )
    }
}

impl Drop for GitDaemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Fixture {
    daemon: GitDaemon,
    _repositories: TempDir,
    project_dir: TempDir,
    shared: TempDir,
    dependency: TempDir,
    replacement: TempDir,
    project: ProjectConfig,
    _cwd: common::CwdGuard,
}

impl Fixture {
    fn new() -> Self {
        let project_dir = TempDir::new().unwrap();
        let repositories = TempDir::new().unwrap();
        let shared = TempDir::new_in(repositories.path()).unwrap();
        let dependency = TempDir::new_in(repositories.path()).unwrap();
        let replacement = TempDir::new_in(repositories.path()).unwrap();
        common::write_local_flake(shared.path(), "shared");
        common::write_local_flake(dependency.path(), "dependency");
        common::write_local_flake(replacement.path(), "replacement");
        fs::write(shared.path().join("devenv.yaml"), "reload: false\n").unwrap();
        commit(shared.path());
        let daemon = GitDaemon::new(repositories.path());
        let shared_url = daemon.url(shared.path());
        let dependency_url = daemon.url(dependency.path());
        fs::write(project_dir.path().join("devenv.nix"), "{ }\n").unwrap();
        fs::write(project_dir.path().join("devenv.yaml"), format!(
            "inputs:\n  nixpkgs:\n    url: {dependency_url}\n  devenv:\n    url: {dependency_url}\n  unrelated:\n    url: {dependency_url}\n  shared:\n    url: {shared_url}\n    flake: false\nimports:\n  - shared\n"
        )).unwrap();
        let project = ProjectConfig::new(project_dir.path());
        let cwd = common::CwdGuard::enter(project_dir.path());
        Self {
            daemon,
            _repositories: repositories,
            project_dir,
            shared,
            dependency,
            replacement,
            project,
            _cwd: cwd,
        }
    }

    fn set_shared(&self, yaml: &str) {
        fs::write(self.shared.path().join("devenv.yaml"), yaml).unwrap();
        commit(self.shared.path());
    }

    fn resolve(&self, update: UpdatePolicy<'_>) -> ResolvedProject {
        resolve_project(&self.project, NixOptions::default(), update).expect("resolve project")
    }

    fn lock_contents(&self) -> String {
        fs::read_to_string(self.project_dir.path().join("devenv.lock")).unwrap()
    }

    fn write(&self, resolved: &ResolvedProject) {
        resolved.write_lock().expect("write completed lock");
    }
}

fn input_node(lock: &str, name: &str) -> Value {
    let lock: Value = serde_json::from_str(lock).unwrap();
    let root = lock["root"].as_str().unwrap();
    let node = lock["nodes"][root]["inputs"][name]
        .as_str()
        .expect("direct root input");
    lock["nodes"][node].clone()
}

#[nix_test]
async fn relative_project_root_resolves_local_flake_subdirectories() {
    let mut fixture = Fixture::new();
    let modules = fixture.project_dir.path().join("src/modules");
    fs::create_dir_all(&modules).unwrap();
    fs::write(modules.join("flake.nix"), "{ outputs = _: {}; }\n").unwrap();
    let yaml_path = fixture.project_dir.path().join("devenv.yaml");
    let yaml = fs::read_to_string(&yaml_path).unwrap().replace(
        &format!(
            "  devenv:\n    url: {}",
            fixture.daemon.url(fixture.dependency.path())
        ),
        "  devenv:\n    url: .?dir=src/modules",
    );
    fs::write(yaml_path, yaml).unwrap();
    fixture.project.root = ".".into();

    let resolved = fixture.resolve(UpdatePolicy::Preserve);
    assert_eq!(resolved.config.reload, Some(false));
    fixture.write(&resolved);
    let before = fixture.lock_contents();
    fixture.write(&fixture.resolve(UpdatePolicy::Input("shared")));
    fixture.write(&fixture.resolve(UpdatePolicy::All));
    assert_eq!(fixture.lock_contents(), before);
}

#[nix_test]
async fn cold_remote_sources_are_materialized_before_loading_yaml() {
    let fixture = Fixture::new();
    // Make a fresh source tree that cannot already exist in the store from
    // another test. Locking the Git revision alone need not materialize it.
    let yaml = format!("reload: false\n# {}\n", fixture.shared.path().display());
    fixture.set_shared(&yaml);
    let resolved = fixture.resolve(UpdatePolicy::Preserve);
    assert_eq!(
        fs::read_to_string(resolved.sources["shared"].join("devenv.yaml")).unwrap(),
        yaml
    );
    assert_eq!(resolved.config.reload, Some(false));
}

#[nix_test]
async fn root_and_nested_follows_can_target_remote_yaml_inputs() {
    let fixture = Fixture::new();
    let dependency_url = fixture.daemon.url(fixture.dependency.path());
    fixture.set_shared(&format!(
        "inputs:\n  imported-pkgs:\n    url: {dependency_url}\n"
    ));
    let root = fixture.project_dir.path().join("devenv.yaml");
    let yaml = fs::read_to_string(&root).unwrap().replace(
        &format!("  nixpkgs:\n    url: {dependency_url}"),
        "  nixpkgs:\n    follows: pkgs-alias\n  pkgs-alias:\n    follows: imported-pkgs",
    ).replace(
        &format!("  unrelated:\n    url: {dependency_url}"),
        &format!("  unrelated:\n    url: {dependency_url}\n    inputs:\n      child:\n        follows: imported-pkgs"),
    );
    // This flake declares the child overridden by the root's nested follows.
    fs::write(
        fixture.dependency.path().join("flake.nix"),
        format!(
            "{{ inputs.child.url = \"{}\"; outputs = {{ self, child }}: {{}}; }}",
            fixture.daemon.url(fixture.replacement.path())
        ),
    )
    .unwrap();
    commit(fixture.dependency.path());
    fs::write(root, yaml).unwrap();

    fixture.write(&fixture.resolve(UpdatePolicy::Preserve));
    let before = fixture.lock_contents();
    let lock: Value = serde_json::from_str(&before).unwrap();
    assert_eq!(
        lock["nodes"]["root"]["inputs"]["nixpkgs"],
        serde_json::json!(["pkgs-alias"])
    );
    assert_eq!(
        input_node(&before, "unrelated")["inputs"]["child"],
        serde_json::json!(["imported-pkgs"])
    );
    // Updating a deferred alias must be postponed until it has a target.
    fixture.write(&fixture.resolve(UpdatePolicy::Input("nixpkgs")));
    fixture.write(&fixture.resolve(UpdatePolicy::All));
    fixture.write(&fixture.resolve(UpdatePolicy::Preserve));
    assert_eq!(fixture.lock_contents(), before);
}

#[nix_test]
async fn imported_alias_can_be_resolved_after_its_remote_target_is_discovered() {
    let fixture = Fixture::new();
    fs::write(
        fixture.replacement.path().join("devenv.yaml"),
        "reload: true\n",
    )
    .unwrap();
    commit(fixture.replacement.path());
    fixture.set_shared(&format!(
        "inputs:\n  config-source:\n    url: {}\n    flake: false\n",
        fixture.daemon.url(fixture.replacement.path())
    ));
    let root = fixture.project_dir.path().join("devenv.yaml");
    let yaml = fs::read_to_string(&root).unwrap().replace(
        "inputs:\n",
        "inputs:\n  alias:\n    follows: config-source\n",
    );
    fs::write(root, format!("{yaml}  - alias\n")).unwrap();

    let resolved = fixture.resolve(UpdatePolicy::Preserve);
    assert_eq!(resolved.config.reload, Some(true));
    assert!(resolved.sources["alias"].join("devenv.yaml").exists());
    fixture.write(&resolved);
    let before = fixture.lock_contents();
    fixture.write(&fixture.resolve(UpdatePolicy::Preserve));
    assert_eq!(fixture.lock_contents(), before);
}

#[nix_test]
async fn unresolved_remote_follows_do_not_replace_the_existing_lock() {
    let fixture = Fixture::new();
    fixture.write(&fixture.resolve(UpdatePolicy::Preserve));
    let before = fixture.lock_contents();
    let root = fixture.project_dir.path().join("devenv.yaml");
    let yaml = fs::read_to_string(&root).unwrap().replace(
        &format!(
            "  nixpkgs:\n    url: {}",
            fixture.daemon.url(fixture.dependency.path())
        ),
        "  nixpkgs:\n    follows: missing-pkgs",
    );
    fs::write(root, yaml).unwrap();
    let error = resolve_project(&fixture.project, NixOptions::default(), UpdatePolicy::All)
        .err()
        .expect("reject follows target absent from complete config");
    assert!(
        error
            .to_string()
            .contains("'nixpkgs' follows missing input 'missing-pkgs'"),
        "{error:?}"
    );
    assert_eq!(fixture.lock_contents(), before);
}

#[nix_test]
async fn update_composes_added_changed_and_removed_inputs_before_writing() {
    let fixture = Fixture::new();
    fixture.set_shared(&format!(
        "inputs:\n  old-tool:\n    url: {}\n",
        fixture.daemon.url(fixture.dependency.path())
    ));
    let initial = fixture.resolve(UpdatePolicy::Preserve);
    assert!(initial.config.inputs.contains_key("old-tool"));
    assert!(!fixture.project_dir.path().join("devenv.lock").exists());
    fixture.write(&initial);
    let before = fixture.lock_contents();

    // The new shared revision replaces one input and adds another. An
    // unrelated moving input must keep its existing pin on selective update.
    common::bump_local_flake(fixture.dependency.path(), "unrelated advanced");
    fixture.set_shared(&format!(
        "inputs:\n  tool:\n    url: {}\n  added-tool:\n    url: {}\n",
        fixture.daemon.url(fixture.dependency.path()),
        fixture.daemon.url(fixture.replacement.path())
    ));
    let updated = fixture.resolve(UpdatePolicy::Input("shared"));
    assert_eq!(
        fixture.lock_contents(),
        before,
        "discovery must not write intermediate locks"
    );
    assert!(!updated.config.inputs.contains_key("old-tool"));
    assert!(updated.config.inputs.contains_key("added-tool"));
    fixture.write(&updated);
    let after = fixture.lock_contents();
    assert!(
        !serde_json::from_str::<Value>(&after).unwrap()["nodes"]["root"]["inputs"]
            .as_object()
            .unwrap()
            .contains_key("old-tool")
    );
    assert_eq!(
        input_node(&before, "unrelated"),
        input_node(&after, "unrelated")
    );
    assert_ne!(input_node(&before, "shared"), input_node(&after, "shared"));

    fixture.set_shared(&format!(
        "inputs:\n  tool:\n    url: {}\n",
        fixture.daemon.url(fixture.replacement.path())
    ));
    let changed = fixture.resolve(UpdatePolicy::Input("shared"));
    fixture.write(&changed);
    let final_lock = fixture.lock_contents();
    assert_ne!(input_node(&after, "tool"), input_node(&final_lock, "tool"));
    assert!(!changed.config.inputs.contains_key("added-tool"));
    fixture.write(&fixture.resolve(UpdatePolicy::Preserve));
    assert_eq!(
        fixture.lock_contents(),
        final_lock,
        "the very next invocation must be a no-op"
    );
}

#[nix_test]
async fn discovery_keeps_transitive_pins_and_reapplies_late_overrides() {
    let mut fixture = Fixture::new();
    fixture.set_shared(&format!(
        "inputs:\n  tool:\n    url: {}\n",
        fixture.daemon.url(fixture.dependency.path())
    ));
    fixture.write(&fixture.resolve(UpdatePolicy::Preserve));
    let before = fixture.lock_contents();
    common::bump_local_flake(fixture.dependency.path(), "new tool revision");
    fixture.write(&fixture.resolve(UpdatePolicy::Preserve));
    assert_eq!(
        fixture.lock_contents(),
        before,
        "incomplete discovery must preserve transitive pins"
    );

    fixture.project.input_overrides = vec![
        "tool".into(),
        fixture.daemon.url(fixture.replacement.path()),
    ];
    let overridden = fixture.resolve(UpdatePolicy::Preserve);
    assert_eq!(
        overridden.config.inputs["tool"].url.as_deref(),
        Some(fixture.project.input_overrides[1].as_str())
    );
    fixture.write(&overridden);
    assert_ne!(
        input_node(&before, "tool"),
        input_node(&fixture.lock_contents(), "tool")
    );
}

#[nix_test]
async fn update_all_updates_inputs_discovered_in_later_passes() {
    let fixture = Fixture::new();
    fixture.set_shared(&format!(
        "inputs:\n  tool:\n    url: {}\n",
        fixture.daemon.url(fixture.dependency.path())
    ));
    fixture.write(&fixture.resolve(UpdatePolicy::Preserve));
    let before = fixture.lock_contents();
    common::bump_local_flake(fixture.dependency.path(), "updated tool");
    let updated = fixture.resolve(UpdatePolicy::All);
    fixture.write(&updated);
    let after = fixture.lock_contents();
    assert_ne!(input_node(&before, "tool"), input_node(&after, "tool"));
    assert_ne!(
        input_node(&before, "unrelated"),
        input_node(&after, "unrelated")
    );
    fixture.write(&fixture.resolve(UpdatePolicy::Preserve));
    assert_eq!(fixture.lock_contents(), after);
}

#[nix_test]
async fn failed_composition_does_not_replace_the_existing_lock() {
    let fixture = Fixture::new();
    fixture.write(&fixture.resolve(UpdatePolicy::Preserve));
    let before = fixture.lock_contents();
    fixture.set_shared("imports:\n  - ../outside-source\n");
    let error = resolve_project(
        &fixture.project,
        NixOptions::default(),
        UpdatePolicy::Input("shared"),
    )
    .err()
    .expect("reject import outside locked source");
    assert!(error.to_string().contains("resolves outside"), "{error:?}");
    assert_eq!(fixture.lock_contents(), before);
}

#[nix_test]
async fn local_input_sources_stay_live_during_composition() {
    let fixture = Fixture::new();
    let yaml_path = fixture.project_dir.path().join("devenv.yaml");
    let yaml = fs::read_to_string(&yaml_path).unwrap().replace(
        &fixture.daemon.url(fixture.shared.path()),
        &format!("path:{}", fixture.shared.path().display()),
    );
    fs::write(yaml_path, yaml).unwrap();

    let first = fixture.resolve(UpdatePolicy::Preserve);
    assert_eq!(
        first.sources["shared"],
        fixture.shared.path().canonicalize().unwrap()
    );
    assert_eq!(first.config.reload, Some(false));
    fixture.write(&first);
    let before = fixture.lock_contents();

    fs::write(fixture.shared.path().join("devenv.yaml"), "reload: true\n").unwrap();
    let second = fixture.resolve(UpdatePolicy::Preserve);
    assert_eq!(second.config.reload, Some(true));
    fixture.write(&second);
    assert_eq!(fixture.lock_contents(), before);
}

#[test]
fn unknown_overrides_fail_without_creating_a_lock() {
    let dir = TempDir::new().unwrap();
    let mut project = ProjectConfig::new(dir.path());
    project.input_overrides = vec!["missing".into(), "github:example/missing".into()];
    assert!(resolve_project(&project, NixOptions::default(), UpdatePolicy::Preserve).is_err());
    assert!(!dir.path().join("devenv.lock").exists());
}
