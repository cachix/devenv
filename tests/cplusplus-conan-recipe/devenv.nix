{ pkgs, config, ... }:
let
  getCommand = package: builtins.baseNameOf (pkgs.lib.getExe package);
in
{
  languages.cplusplus = {
    enable = true;
    conan = {
      enable = true;
      install.enable = true;
      config = {
        profiles.settings.build_type = "Release";
        compilerCppStd = "17";
        remotes.local = {
          url = "./repo";
          local = true;
          allowedPackages = [
            "hello-world/0.0.1.cci.20260428"
          ];
        };
        offline = true;
      };
    };
  };
  enterTest = ''
    ${getCommand config.languages.cplusplus.conan.package} create . --build=missing 2>&1 \
      | grep "example/0.0.1"
    ${getCommand config.languages.cplusplus.conan.package} remote list \
      | grep "local-recipes-index, Enabled: True, Allowed packages: hello-world/0.0.1.cci.20260428"
  '';
}
