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
        stdenv = pkgs.overrideCC
          (
            pkgs.llvmPackages.libcxxStdenv.override {
              targetPlatform.useLLVM = true;
            }
          )
          pkgs.llvmPackages.clangUseLLVM;
        # By default: compiler.libcxx=libstdc++11, so undo it:
        compilerLibCxx = null;
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
