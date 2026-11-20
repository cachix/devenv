# Maintained design views

Maintainers of this fork own this provider configuration view. Existing CI uses GitHub Actions; this adds a separate lightweight PR job without changing build/test/release jobs. The audit found generic provider module tests and upstream documentation; no maintained Atlas integration diagram or PR template. This policy applies to every PR to this maintained fork. It does not impose Atlas execution instructions or this policy on unrelated upstream repositories.

These contributor references were added against the 2.4.1 source line; they are
plain repository Markdown outside the MDX documentation site and introduce no
runtime feature or module behavior change.

## Source-to-visual coverage

| Visual / purpose | Source coverage and review owner |
| --- | --- |
| [What do provider integration modules own?](provider-config.md) | provider integration modules, files, native provider fixtures; fork integration maintainers |

Module/API links and the owner of each edge appear in each view's source table.
Source baselines in review provenance identify the audited implementation, not
an installed production version. For changes outside this map, extend the map
and add a focused view or explain the verified relationship to an existing view.

## Every pull request

Every PR to this repository must update relevant diagram source **and** include
an implementation/diagram mapping in the PR body. There is no “not applicable”
exception, including documentation, dependency, test and policy PRs. When topology
stays unchanged, update the affected view's **Review provenance** with the changed
source paths, invariant checked, final source revision or PR context, and why the
view remains accurate. For policy-only work, maintain this index's policy coverage
below. Record actual review, not a date-only or cosmetic edit. Change the diagram
itself whenever behavior, ownership, states or boundaries change.

Use the PR template's `diagram-maintenance` JSON record. Every changed path outside
`docs/design/` must occur in at least one mapping, and every mapping must reference
a changed file in `docs/design/`. Include changed design paths too when they are
the implementation being reviewed. All paths are repository-relative. Explain
what was compared in `reason`; include renderer version, preview results and
legibility checks in `validation`. These are author attestations pending review.

The reviewer compares the **final implementation diff** with each mapped visual,
checks the source links and captions, distinguishes proposed behavior from current
behavior, and inspects the rendered labels/edges at readable scale. Automated
checks cannot prove semantic fidelity. Do not accept or complete a task until this
comparison is recorded against its exact final commits; rerun review after fixes.
This rule applies to future lifecycle/teardown, write-path and messaging PRs too.

## Preview and checks

Use docu.md Markdown Viewer or export with the pinned, tested CLI:

```sh
npx --yes @markdown-viewer/documd@6.0.0 docs/design/VIEW.md --assets /tmp/design-preview --format svg
python3 scripts/check_design.py --base BASE_COMMIT --body /tmp/pr-body.md
python3 scripts/test_design.py
```

`VIEW.md` means one of the views above. Repeat the render for every changed view;
use external scratch/output directories. The source uses the documd-visuals
Default theme and supported `plantuml` fences. GitHub's default Markdown view
shows those fences as source; stock Obsidian rendering is not claimed. The
documented render path is docu.md / documd, not a GitHub image asset tree.

The `Design maintenance` GitHub Actions workflow runs on every PR (including body
edits), validates the record, changed source mapping, supported fences and local
links, exercises negative controls, and renders all views with documd 6.0.0, rejecting empty SVG output.
Syntax checks cover fences, UML delimiters and balanced braces; the permissive
renderer is not a complete PlantUML language validator.
This is repository-local CI once merged. No organization-wide enforcement,
required branch-protection status, installed runtime or activated host changes
are implied. Merge settings remain operator-owned.

## Policy coverage and provenance

This index covers contributor/agent instructions, the PR template, the design
checker/tests and its workflow. Their invariant is that every PR has a meaningful
mapping to maintained visual source and independent semantic review before
acceptance. Initial policy implementation adds that contract; CI is structural
and rendering evidence, while reviewers own semantic verification.

Preview validation also checks labelled SVG foreignObject output, which is how
documd represents UML text; a valid HTML label must not be mistaken for an empty
SVG. An actual empty render is a rejected negative control.
