{ pkgs, config, ... }:

{
  stdenv = pkgs.stdenvNoCC;
  languages.cplusplus.enable = true;
  languages.cplusplus.conan.enable = true;
  languages.cplusplus.conan.install.enable = true;
  enterTest = ''
    ${pkgs.lib.getExe config.languages.cplusplus.package} --version
    ${pkgs.lib.getExe config.languages.cplusplus.package} --version \
      | grep ${pkgs.lib.escapeShellArg pkgs.stdenv.cc.cc.pname}
    ${pkgs.lib.getExe config.languages.cplusplus.cmake.package} --version
    ${pkgs.lib.getExe config.languages.cplusplus.lsp.package} --version \
      | grep ${pkgs.lib.escapeShellArg config.languages.cplusplus.lsp.package.version}
    ${pkgs.lib.getExe config.languages.cplusplus.conan.package} --version
    echo "enable:"${pkgs.lib.escapeShellArg config.languages.cplusplus.tools.enable}":" \
      | grep "enable:"${pkgs.lib.escapeShellArg pkgs.stdenv.cc.isClang}":"
    ${pkgs.lib.getExe config.languages.cplusplus.conan.package} profile show \
      | grep "cmake/"${pkgs.lib.escapeShellArg config.languages.cplusplus.cmake.package.version}
  '';
}
