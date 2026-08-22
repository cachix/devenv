{ pkgs, config, ... }:
{
  languages.cplusplus = {
    enable = true;
    conan = {
      enable = true;
      install.enable = true;
      config = {
        profiles.default = {
          settings."compiler.cppstd" = "14";
          settings.build_type = "Debug";
        };
      };
    };
  };
}
