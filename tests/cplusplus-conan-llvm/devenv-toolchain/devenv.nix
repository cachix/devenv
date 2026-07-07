{ pkgs, config, lib, ... }:
let
  inherit (lib) getExe;
  getCommand = package: builtins.baseNameOf (getExe package);
  cfg = config.languages.cplusplus;
in
{
  stdenv = pkgs.overrideCC
    (
      pkgs.llvmPackages.libcxxStdenv.override {
        targetPlatform.useLLVM = true;
        targetPlatform.linker = "lld";
      }
    )
    pkgs.llvmPackages.clangUseLLVM;

  languages.cplusplus = {
    enable = true;
    conan = {
      enable = true;
      install.enable = true;
      config = {
        profiles = {
          settings._.build_type = "Release";
        };
        remotes.local = {
          url = "./repo";
          local = true;
          allowedPackages = [ "hello-world/0.0.1.cci.20260428" ];
        };
        offline = true;
      };
    };
  };

  enterTest = ''
    # Check "libstdc++11" wasn't used:
    ! ${getCommand cfg.conan.package} create . --build=missing 2>&1 \
      | grep -F "_GLIBCXX_USE_CXX11_ABI 1"
  '';
}
