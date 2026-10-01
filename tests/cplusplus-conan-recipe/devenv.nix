{ pkgs, config, ... }:
let
  getCommand = package: baseNameOf (pkgs.lib.getExe package);
  cfg = config.languages.cplusplus;
in
{
  languages.cplusplus = {
    enable = true;
    conan = {
      enable = true;
      install.enable = true;
      config = {
        profiles.default = {
          settings."compiler.cppstd" = "17";
          settings.build_type = "Release";
        };
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
    ${getCommand cfg.conan.package} create . --build=missing 2>&1 \
      | grep "example/0.0.1"
    ${getCommand cfg.conan.package} remote list \
      | grep "local-recipes-index, Enabled: True, Allowed packages: hello-world/0.0.1.cci.20260428"
  '';
}
