{ pkgs, ... }:

{
  conan = {
    enable = true;
    config = {
      platformToolRequires = {
        cmake = pkgs.cmake.version;
      };
      devShell = {
        packages = [
          pkgs.cmake
        ];
      };
    };
  };

  enterTest = ''
    conan profile show | grep "cmake/"${pkgs.lib.escapeShellArg pkgs.cmake.version}
  '';
}
