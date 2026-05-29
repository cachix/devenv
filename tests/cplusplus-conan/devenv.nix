{ pkgs, config, ... }:
let
  getCommand = package: builtins.baseNameOf (pkgs.lib.getExe package);
in
{
  languages.cplusplus.enable = true;
  languages.cplusplus.conan.enable = true;
  languages.cplusplus.conan.install.enable = true;
  enterTest = ''
    ${getCommand config.languages.cplusplus.package} --version
    ${getCommand config.languages.cplusplus.package} --version \
      | grep ${pkgs.lib.escapeShellArg config.stdenv.cc.cc.pname}
    ${getCommand config.languages.cplusplus.cmake.package} --version
    ${getCommand config.languages.cplusplus.lsp.package} --version \
      | grep ${pkgs.lib.escapeShellArg config.languages.cplusplus.lsp.package.version}
    ${getCommand config.languages.cplusplus.conan.package} --version
    echo "enable:"${pkgs.lib.escapeShellArg config.languages.cplusplus.tools.enable}":" \
      | grep "enable:"${pkgs.lib.escapeShellArg config.stdenv.cc.isClang}":"
    ${getCommand config.languages.cplusplus.conan.package} profile show \
      | grep "cmake/"${pkgs.lib.escapeShellArg config.languages.cplusplus.cmake.package.version}
  '';
}
