{ config, pkgs, inputs, ... }:

{
  languages.rust = {
    enable = true;
    # https://devenv.sh/reference/options/#languagesrustversion
    version = "latest";
  };

  pre-commit.hooks = {
    clippy.enable = true;
    rustfmt.enable = true;
  };

  env.RUST_SRC_PATH = "${inputs.fenix.packages.${pkgs.system}.${config.languages.rust.version}.rust-src}/lib/rustlib/src/rust/library";
}
