# Claude Code `FileChanged` hook - filtering matcher

Demonstrates the second role of a `claude.code.hooks.*.matcher` for a
`FileChanged` hook, per the
[Claude Code docs](https://code.claude.com/docs/en/hooks#filechanged):

> The matcher for this event serves two roles:
>
> - Build the watch list: the value is split on `|` and each segment is
>   registered as a literal filename in the working directory.
> - Filter which hooks run: when a watched file changes, the same value
>   filters which hook groups run using the standard matcher rules against
>   the changed file's basename.

This example configures two `FileChanged` hooks whose watch lists overlap:

- `notify-env-change` watches `.env`, `.env.local` and `.env.production`,
  and logs a generic "environment file changed" notice.
- `warn-production-env` watches only `.env.production`, and additionally
  logs an extra warning - but only for that one file.

Both hooks contribute to one combined, per-project watch list (role 1). But
when a specific file actually changes, each hook's *own* matcher decides
whether *that* hook fires for *that* file (role 2):

| File that changed | `notify-env-change` | `warn-production-env` |
| --- | --- | --- |
| `.env` | fires | does not fire |
| `.env.local` | fires | does not fire |
| `.env.production` | fires | fires |

Without this filtering step, changing `.env` would spuriously run
`warn-production-env`'s command too, just because `.env` happens to share a
watch list with `.env.production`.

`.test.sh` checks the real generated `.claude/settings.json` (part 1), and
separately re-implements the documented filtering rule to walk through the
table above (part 2) - deterministically, without needing a live `claude`
session or credentials.

See [`claude-filechanged-basic`](../claude-filechanged-basic) and
[`claude-filechanged-multi`](../claude-filechanged-multi) for the first
role (building the watch list) in isolation.
