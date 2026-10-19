{
  config,
  lib,
  options,
  pkgs,
  ...
}:

let
  engineType = options.dagger.engine.type;
  runtimes = [
    "docker"
    "apple"
    "podman"
    "finch"
    "nerdctl"
  ];
  validEngines = [
    null
    "cloud"
    "image://registry.example/engine:latest"
    "container://dagger"
    "tcp://localhost:1234"
    "tls://localhost"
    "tls://localhost:1234"
    "ssh://localhost"
    "ssh://user@localhost:22"
    "kube-pod://dagger"
    "unix:///var/run/dagger.sock"
    "docker-image://dagger:latest"
    "docker-container://dagger"
  ]
  ++ lib.concatMap (runtime: [
    "image+${runtime}://dagger:latest"
    "container+${runtime}://dagger"
  ]) runtimes;
  invalidEngines = [
    ""
    "other"
    "https://localhost"
    "image+unknown://dagger"
    "container+unknown://dagger"
    "image://"
    "container://"
    "cloud://engine"
    123
  ];
in
{
  dagger = {
    enable = true;
    package = pkgs.hello;
  };

  assertions = [
    {
      assertion = !config.dagger.dang.enable && !config.languages.dang.enable;
      message = "Dagger should leave Dang disabled by default.";
    }
    {
      assertion = !(builtins.elem config.languages.dang.package config.packages);
      message = "Dagger should not install Dang by default.";
    }
    {
      assertion = config.dagger.version == null && config.dagger.engine == null;
      message = "Dagger version and engine should default to null.";
    }
    {
      assertion = !(config.env ? DAGGER_X_RELEASE) && !(config.env ? DAGGER_ENGINE);
      message = "Null Dagger options should not export environment variables.";
    }
    {
      assertion = config.env.DO_NOT_TRACK == "1";
      message = "Dagger should disable tracking.";
    }
    {
      assertion = builtins.all engineType.check validEngines;
      message = "dagger.engine should accept all supported engine schemes and runtimes.";
    }
    {
      assertion = builtins.all (engine: !engineType.check engine) invalidEngines;
      message = "dagger.engine should reject unsupported schemes, empty targets, and non-strings.";
    }
    {
      assertion = config.dagger.package == pkgs.hello;
      message = "dagger.package should use the configured package.";
    }
    {
      assertion = builtins.elem config.dagger.package config.packages;
      message = "dagger.enable should add dagger.package to packages.";
    }
  ];

  enterTest = ''
    set -euo pipefail
    test "$DO_NOT_TRACK" = "1"
  '';
}
