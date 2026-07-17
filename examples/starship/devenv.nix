{ pkgs, ... }:

{
  starship.enable = true;

  # Set `config.enable` to true to override Starship's default configuration.
  # With it set to `false`, Starship will still look for a configuration
  # file in '~/.config/starship.toml'.
  starship.config.enable = true;

  # Store your configuration within devenv.nix under `config.settings`.
  starship.config.settings = {
    add_newline = false;
    character = {
      success_symbol = "[➜](bold green)";
    };
  };

  # Alternatively, point `config.path` at a Starship configuration file.
  #starship.config.path = ./starship.toml;
}
