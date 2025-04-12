### Release on GitHub

- `git commit`
- `git tag`
- `git push --tags`
- Create a release on GitHub
- Bump dev version in devenv/Cargo.toml and package.nix

### Release on nixpkgs

- Sync the `package.nix` in nixpkgs with [./package.nix](./package.nix)
- Update `devenv_nix` if necessary

### After nixpkgs release

Wait for the release to reach `nixpkgs-unstable`.

- Write a blog post
- Update [`src/modules/latest-version`](./src/modules/latest-version)
