{ pkgs, config, inputs, ... }:
{
  languages.python = {
    enable = true;
    directory = "./directory";
    venv.enable = true;
    uv = {
      enable = true;
      package = pkgs.uv;
      sync.enable = true;
    };
  };
}
