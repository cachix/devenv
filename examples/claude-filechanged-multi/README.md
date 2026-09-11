# Claude Code `FileChanged` hook - `|`-separated filenames

Demonstrates the `|`-separated case of a `claude.code.hooks.*.matcher` for a
`FileChanged` hook: a value like `.env|.env.local`.

Per the [Claude Code docs](https://code.claude.com/docs/en/hooks#filechanged),
building the `FileChanged` watch list splits the matcher on `|` and
registers each segment as a *literal* filename relative to the project
root. `.env|.env.local` therefore watches exactly two files - `.env` and
`.env.local` - and nothing that merely looks similar (`.env.production`
would not be watched by this matcher).

See [`claude-filechanged-basic`](../claude-filechanged-basic) for the
single-filename case, and
[`claude-filechanged-filter`](../claude-filechanged-filter) for how the same
matcher value is reused to filter which of several `FileChanged` hooks
actually run once a watched file changes.
