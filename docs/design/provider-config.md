# What do provider integration modules own?

These generic modules materialize native workspace configuration. They do not implement Atlas Session state, wrapper finalization or Atlas teardown. This scope belongs to the maintained fork; it does not impose Atlas runtime policy on upstream projects.

```plantuml
@startuml
skinparam RectangleBackgroundColor #eef2fb
skinparam RectangleBorderColor #5b6b8c
skinparam RectangleFontColor #1f2937
skinparam ComponentBackgroundColor #eef2fb
skinparam ComponentBorderColor #5b6b8c
skinparam ComponentFontColor #1f2937
skinparam ClassBackgroundColor #eef2fb
skinparam ClassBorderColor #5b6b8c
skinparam ClassFontColor #1f2937
skinparam UsecaseBackgroundColor #eef2fb
skinparam UsecaseBorderColor #5b6b8c
skinparam UsecaseFontColor #1f2937
skinparam DatabaseBackgroundColor #eef2fb
skinparam DatabaseBorderColor #5b6b8c
skinparam DatabaseFontColor #1f2937
skinparam NodeBackgroundColor #eef2fb
skinparam NodeBorderColor #5b6b8c
skinparam NodeFontColor #1f2937
skinparam ActorBackgroundColor #eef2fb
skinparam ActorBorderColor #5b6b8c
skinparam ActorFontColor #1f2937
skinparam StateBackgroundColor #eef2fb
skinparam StateBorderColor #5b6b8c
skinparam StateFontColor #1f2937
skinparam ArtifactBackgroundColor #eef2fb
skinparam ArtifactBorderColor #5b6b8c
skinparam ArtifactFontColor #1f2937
skinparam CloudBackgroundColor #eef2fb
skinparam CloudBorderColor #5b6b8c
skinparam CloudFontColor #1f2937
skinparam FolderBackgroundColor #eef2fb
skinparam FolderBorderColor #5b6b8c
skinparam FolderFontColor #1f2937
skinparam PackageBackgroundColor #eef2fb
skinparam PackageBorderColor #5b6b8c
skinparam DefaultFontColor #1f2937
skinparam ArrowColor #5b6b8c
skinparam ArrowFontColor #1f2937
skinparam NoteBackgroundColor #dfe5fb
skinparam NoteBorderColor #5b6b8c
skinparam NoteFontColor #1f2937
skinparam stereotypeABackgroundColor #d9e3f4
skinparam stereotypeABorderColor #5b6b8c
skinparam stereotypeCBackgroundColor #d9e3f4
skinparam stereotypeCBorderColor #5b6b8c
skinparam stereotypeEBackgroundColor #d9e3f4
skinparam stereotypeEBorderColor #5b6b8c
skinparam stereotypeIBackgroundColor #d9e3f4
skinparam stereotypeIBorderColor #5b6b8c
component "Devenv module configuration" as config
component "codex.nix" as codex
component "claude.nix" as claude
component "files module" as files
component "Codex workspace settings / agents / skills" as cx
component "Claude workspace settings / agents / skills" as cl
config --> codex : codex options
config --> claude : claude options
codex --> files : native TOML / Markdown / resources
claude --> files : native JSON / Markdown / resources
files --> cx : materialize selected files
files --> cl : materialize selected files
@enduml
```

## Source and edge ownership

| Edge | Source / owner |
| --- | --- |
| Codex settings, hook grouping, agents and selected skills | [codex.nix](../../src/modules/integrations/codex.nix) |
| Claude native settings, agents and skills | [claude.nix](../../src/modules/integrations/claude.nix) |
| Workspace materialization and copy mode | [files.nix](../../src/modules/files.nix) |
| Native module behavior fixtures | [Codex integration tests](../../tests/codex/devenv.nix), [Claude integration tests](../../tests/claude-skills/devenv.nix) |

Atlas selects one provider per environment through its external vault-owned Devenv configuration. Generic provider hook configuration here is distinct from launch-scoped hooks instrumented by the [runtime wrapper](https://github.com/frisitano/atlas-runtime/blob/main/src/atlas_runtime/wrapper.py). A provider `SessionEnd` does not mean Atlas process finalization. Devenv-owned Atlas session lifecycle and wrapper-free teardown remain proposed follow-up work. This diagram does not assert deployment of the checked-out fork.

## Review provenance

Initial source audit: 1f8307d1. This view describes repository source, not proof of installed activation.
