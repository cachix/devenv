{ pkgs, config, lib, ... }:
let
  inherit (lib) getExe;
  getCommand = package: builtins.baseNameOf (getExe package);
  cfg = config.languages.cplusplus.conan.config;
  c = "'c': '${getExe cfg.stdenv.cc}'";
  cpp = "'cpp': '${builtins.dirOf (getExe cfg.stdenv.cc)}/clang++'";
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
          settings.build_type = "Release";
          conf = {
            "tools.build:compiler_executables" = "{${c}, ${cpp}}";
          };
        };
        # By default: compiler.libcxx=libstdc++11, so set it:
        compilerLibCxx = "libc++";
      };
    };
  };

  enterTest = ''
    ${getCommand config.languages.cplusplus.package} --version
    ${getCommand config.languages.cplusplus.package} --version \
      | grep clang
    ${getCommand config.languages.cplusplus.cmake.package} --version
    ${getCommand config.languages.cplusplus.lsp.package} --version \
      | grep ${pkgs.lib.escapeShellArg config.languages.cplusplus.lsp.package.version}
    ${getCommand config.languages.cplusplus.conan.package} --version
    echo "enable:"${pkgs.lib.escapeShellArg config.languages.cplusplus.tools.enable}":" | grep "enable:1:"
    ${getCommand config.languages.cplusplus.conan.package} profile show \
      | grep "cmake/"${pkgs.lib.escapeShellArg config.languages.cplusplus.cmake.package.version}
  '';
}
