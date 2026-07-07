{ pkgs, config, ... }:
{
  languages.cplusplus = {
    enable = true;
    conan = {
      enable = true;
      install.enable = true;
      config = {
        profiles = {
          settings.compiler."compiler.cppstd" = "14";
          settings._.build_type = "Debug";
        };
      };
    };
  };
}
