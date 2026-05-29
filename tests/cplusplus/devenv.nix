{ pkgs, config, ... }:
let
  getCommand = package: builtins.baseNameOf (pkgs.lib.getExe package);
in
{
  languages.cplusplus.enable = true;
  enterTest = ''
    clang --version
    ${getCommand config.languages.cplusplus.package} --version \
      | grep clang
    cmake --version
    ccls --version | grep ${pkgs.lib.escapeShellArg config.languages.cplusplus.lsp.package.version}
    # Validate some clang-tools are in the path:
    clang-doc --version
    clang-format --version
    clang-tidy --version
  '';
}
