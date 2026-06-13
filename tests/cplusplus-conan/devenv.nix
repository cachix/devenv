{ pkgs, config, ... }:
let
  getCommand = package: builtins.baseNameOf (pkgs.lib.getExe package);
  cfg = config.languages.cplusplus;
in
{
  languages.cplusplus = {
    enable = true;
    conan = {
      enable = true;
      install.enable = false;
      config = {
        devShell = {
          env.SOME_ENV_VAR = "SOME_VAL";
          tools = {
            inherit (pkgs) hello;
          };
        };
      };
    };
  };
  enterTest = ''
    # Check commands are added to the path:
    ${getCommand cfg.package} --version
    ${getCommand cfg.cmake.package} --version
    ${getCommand cfg.lsp.package} --version
    ${getCommand cfg.conan.package} --version

    # Check custom additions:
    echo $SOME_ENV_VAR | grep -F "SOME_VAL"
    hello --version
  '';
  assertions = [
    {
      assertion = cfg.package.isClang -> cfg.tools.enable;
      message = "languages.cplusplus.tools should be enabled for clang compilers by default.";
    }
    {
      assertion = cfg.cmake.package == pkgs.cmake;
      message = "by default languages.cplusplus.cmake.package == pkgs.cmake (pkgs.cmake: ${pkgs.cmake.meta.name}). Got: ${cfg.cmake.package.meta.name}";
    }
    {
      assertion = cfg.conan.package == pkgs.conan;
      message = "by default languages.cplusplus.conan.package == pkgs.conan (pkgs.conan: ${pkgs.conan.meta.name}). Got: ${cfg.conan.package.meta.name}";
    }
    {
      assertion = cfg.conan.config.stdenv == config.stdenv;
      message = "by default languages.cplusplus.conan.config.stdenv == stdenv when languages.cplusplus.conan.enable.";
    }
    {
      assertion = cfg.package == cfg.conan.config.stdenv.cc;
      message = "by default languages.cplusplus.package == languages.cplusplus.conan.config.stdenv.cc when languages.cplusplus.conan.enable (languages.cplusplus.conan.config.stdenv.cc: ${cfg.conan.config.stdenv.cc.meta.name}). Got: ${cfg.package.meta.name}";
    }
  ];
}
