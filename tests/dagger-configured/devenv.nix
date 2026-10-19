{
  config,
  options,
  pkgs,
  ...
}:

{
  dagger = {
    enable = true;
    package = pkgs.hello;
    dang.enable = true;
    version = "v0.20.0";
    engine = "cloud";
  };

  assertions = [
    {
      assertion = config.languages.dang.enable;
      message = "dagger.dang.enable should enable languages.dang.";
    }
    {
      assertion = config.languages.dang.package == options.languages.dang.package.default;
      message = "Dagger should keep the default Dang package.";
    }
    {
      assertion = builtins.elem config.languages.dang.package config.packages;
      message = "dagger.dang.enable should install Dang.";
    }
    {
      assertion = config.env.DAGGER_X_RELEASE == config.dagger.version;
      message = "dagger.version should be exported as DAGGER_X_RELEASE.";
    }
    {
      assertion = config.env.DAGGER_ENGINE == config.dagger.engine;
      message = "dagger.engine should be exported as DAGGER_ENGINE.";
    }
  ];

  enterTest = ''
    set -euo pipefail
    test "$DO_NOT_TRACK" = "1"
    test "$DAGGER_X_RELEASE" = "v0.20.0"
    test "$DAGGER_ENGINE" = "cloud"
    command -v dang >/dev/null
  '';
}
