### Release on GitHub

- Update `devenv_nix` if necessary and run all tests
- Tag a new release using https://github.com/cachix/devenv/releases/new

### Release on nixpkgs

- Sync the `package.nix` in nixpkgs with [./package.nix](./package.nix) and bump `devenv_nix` if necessary

### After nixpkgs release

- Write a blog post
