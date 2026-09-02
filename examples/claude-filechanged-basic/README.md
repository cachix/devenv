# Claude Code `FileChanged` hook - single filename

Demonstrates the base case of a `claude.code.hooks.*.matcher` for a
`FileChanged` hook: a value with no `|` in it, naming exactly one file.

Per the [Claude Code docs](https://code.claude.com/docs/en/hooks#filechanged),
building the `FileChanged` watch list never expands globs or regexes - the
matcher is split on `|` and each resulting segment is registered as a
*literal* filename relative to the project root. With a single segment like
`.envrc`, that means: watch the one file named exactly `.envrc`.

See [`claude-filechanged-multi`](../claude-filechanged-multi) for the
`|`-separated, multi-file case, and
[`claude-filechanged-filter`](../claude-filechanged-filter) for how the same
matcher value is reused to filter which of several `FileChanged` hooks
actually run once a watched file changes.
