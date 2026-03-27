{
  pkgs,
  config,
  inputs,
  ...
}:
{
  languages.python = {
    enable = true;
    venv.enable = true;
    uv = {
      enable = true;
      sync = {
        enable = true;
        allGroups = true;
      };
    };
  };
}
