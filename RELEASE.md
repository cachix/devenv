### Release on GitHub

- `git tag vX.X`
- `git push --tags`
- Create a release on GitHub
- Bump minor version in Cargo.toml and package.nix
- Run `cargo update --workspace` to sync Cargo.lock with the new version
- `git commit`

### Release on nixpkgs

- Sync the `package.nix` in nixpkgs with [./package.nix](./package.nix)
- Update `devenv_nix` if necessary

### After nixpkgs release

Wait for the release to reach `nixpkgs-unstable`.

- Write a blog post
- Update [`src/modules/latest-version`](./src/modules/latest-version)
