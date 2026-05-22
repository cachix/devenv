{ pkgs, config, ... }:

{
  stdenv = pkgs.overrideCC
    (
      pkgs.llvmPackages.libcxxStdenv.override {
        targetPlatform.useLLVM = true;
      }
    )
    pkgs.llvmPackages.clangUseLLVM;

  languages.cplusplus = {
    enable = true;
    conan = {
      enable = true;
      install.enable = true;
      config = {
        # By default: compiler.libcxx=libstdc++11, so undo it:
        compilerLibCxx = null;
      };
    };
  };

  enterTest = ''
    ${pkgs.lib.getExe config.languages.cplusplus.package} --version
    ${pkgs.lib.getExe config.languages.cplusplus.cmake.package} --version
    ${pkgs.lib.getExe config.languages.cplusplus.conan.package} --version
    echo "enable:"${pkgs.lib.escapeShellArg config.languages.cplusplus.tools.enable}":" | grep "enable:1:"
    ${pkgs.lib.getExe config.languages.cplusplus.conan.package} profile show | grep "cmake/"${pkgs.lib.escapeShellArg config.languages.cplusplus.cmake.package.version}
  '';
}
