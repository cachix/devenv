{ pkgs, lib, config, ... }:

{
  languages.rust.enable = true;
  languages.rust.channel = lib.mkDefault "stable";

  packages = [ (pkgs.writeShellScriptBin "devenv-test-existing-pkg" "echo existing") ];

  env = {
    RUST_VERSION = config.languages.rust.channel;
    REDIS_ENABLED = builtins.toString config.services.redis.enable;
  };
}
