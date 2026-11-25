## Change

Describe the concrete problem and resulting behavior.

<!--
For dependency updates, list the old and new versions and the focused checks you ran.
Keep generated dependency definitions in sync with lockfiles. For the Ruby LSP,
update `src/modules/lib/ruby-lsp/gemset.nix` alongside `Gemfile.lock` and verify
that Nix resolves the new gem version.
Report any pending checks. If CI failures also occur on the target branch, link
to the matching runs.
-->

## Diagram maintenance

Replace all example values. Add mappings until every changed implementation,
documentation, dependency, test and policy path is covered. Each mapped design
file must be changed in this PR. For unchanged topology, record meaningful source
review provenance in that view (or index for policy), with the invariant checked.

<!-- diagram-maintenance
{
  "mappings": [
    {
      "sources": ["path/to/changed/source"],
      "visual": "docs/design/VIEW.md",
      "reason": "Explain final implementation comparison and diagram/provenance update"
    }
  ],
  "validation": "Renderer version, all preview results and legibility inspection"
}
diagram-maintenance -->

## Reviewer verification

Reviewer: compare the final code and mapped rendered diagrams, verify source
links, states and owners, and record the reviewed commit IDs and findings in your
review. Resolve findings and review the final commits again before acceptance.
