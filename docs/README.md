# Documentation

The [devenv.sh](https://devenv.sh) documentation, built with [MkDocs](https://www.mkdocs.org/) and [Material for MkDocs](https://squidfunk.github.io/mkdocs-material/).

## Local development

```bash
devenv up docs
```

This generates option docs, then starts `mkdocs serve` with live reload.

## Structure

- `src/` — documentation source files (human-written)
- `src/_generated/` — auto-generated option docs (gitignored, populated at build time)
- `src/languages/`, `src/services/`, `src/supported-process-managers/` — module docs that inline generated options via [pymdownx.snippets](https://facelessuser.github.io/pymdown-extensions/extensions/snippets/)
- `scripts/` — doc generation scripts
- `filterOptions.nix` — NixOS module option filtering utility

## Tasks

Doc generation runs automatically before `devenv up docs`:

- `docs:generate-doc-options` — generates the full options reference in `src/_generated/`
- `docs:generate-option-docs` — generates per-module option docs in `src/_generated/`

Scripts available in the devenv shell:

- `devenv-verify-module-docs` — creates doc files for new modules

## Adding documentation for a new module

1. Add a new module in `src/modules/`
2. Run `devenv shell devenv-verify-module-docs` to create the doc file
3. Edit the generated file in `src/languages/`, `src/services/`, or `src/supported-process-managers/` to add guides and examples above the snippet include

## Deployment

Docs are built and deployed to [Cloudflare Pages](https://pages.cloudflare.com/) via GitHub Actions (`.github/workflows/docs.yml`).

- **Production**: pushes to `main` deploy to [devenv.sh](https://devenv.sh)
- **PR previews**: pull requests get a preview URL posted as a comment

### Required secrets

Configure these in GitHub repo settings (Settings > Secrets and variables > Actions):

| Secret | Description |
|---|---|
| `CLOUDFLARE_API_TOKEN` | API token from [Cloudflare dashboard](https://dash.cloudflare.com/profile/api-tokens) |
| `CLOUDFLARE_ACCOUNT_ID` | Account ID from the Cloudflare dashboard sidebar |

### Creating the API token

1. Go to https://dash.cloudflare.com/profile/api-tokens
2. Click **Create Token**
3. Use the **"Edit Cloudflare Workers"** template, or create a custom token with:
   - **Cloudflare Pages: Edit**
   - **Account Settings: Read**
4. Scope the token to your account
