{ pkgs, config, ... }:
let
  cfg = config.languages.cplusplus;
in
{
  languages.cplusplus.enable = true;
  languages.cplusplus.conan.enable = false;
  languages.cplusplus.conan.install.enable = false;
  enterTest = ''
    # Check CMake, clang, some clang-tools and ccls are added to the path:
    cmake --version
    clang --version
    clang-doc --version
    clang-format --version
    clang-tidy --version
    ccls --version
  '';
  assertions = [
    {
      assertion = cfg.package == pkgs.clang;
      message = "by default languages.cplusplus.package == pkgs.clang (pkgs.clang: ${pkgs.clang.meta.name}). Got: ${cfg.package.meta.name}";
    }
    {
      assertion = cfg.cmake.package == pkgs.cmake;
      message = "by default languages.cplusplus.cmake.package == pkgs.cmake (pkgs.cmake: ${pkgs.cmake.meta.name}). Got: ${cfg.cmake.package.meta.name}";
    }
    {
      assertion = cfg.tools.enable;
      message = "languages.cplusplus.tools should be enabled by default.";
    }
    {
      assertion = cfg.tools.package == pkgs.clang-tools;
      message = "by default languages.cplusplus.tools.package == pkgs.clang-tools (pkgs.clang-tools: ${pkgs.clang-tools.meta.name}). Got: ${cfg.tools.package.meta.name}";
    }
    {
      assertion = cfg.lsp.enable;
      message = "languages.cplusplus.lsp should be enabled by default.";
    }
    {
      assertion = cfg.lsp.package == pkgs.ccls;
      message = "by default languages.cplusplus.lsp.package == pkgs.ccls (pkgs.ccls: ${pkgs.ccls.meta.name}). Got: ${cfg.lsp.package.meta.name}";
    }
  ];
}
