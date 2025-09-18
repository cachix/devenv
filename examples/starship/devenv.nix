{ pkgs, ... }:

{
  starship.enable = true;

  # You can change `enableConfig` to true to override Starship default values.
  # By default, it looks for a configuration file next to your `devenv.yaml`. 
  #starship.config.enable = true;
  # If you don't want to place your configuration file next to your `devenv.yaml`,
  # change `config.path` to point to the Starship configuration file you want to use.
  #starship.config.path = ~/.config/custom_starship.toml;
  # If you want to store your configuration within devenv.nix
  # add it under config.settings
  #starship.config.settings = {
  #  add_newline = true;
  #}

  # With `enableConfig` set to `false`, Starship will still look for a configuration
  # file in '~/.config/starship.toml'.
}
