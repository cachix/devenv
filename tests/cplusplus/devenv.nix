{ pkgs, config, ... }:

{
  languages.cplusplus.enable = true;
  enterTest = ''
    clang --version
    cmake --version
    ccls --version | grep ${pkgs.lib.escapeShellArg config.languages.cplusplus.lsp.package.version}
    # Validate some clang-tools are in the path:
    clang-doc --version
    clang-format --version
    clang-tidy --version
  '';
}
