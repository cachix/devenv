{ pkgs, lib, config, ... }:
{
  # Add some test changelog entries
  changelogs = [
    {
      date = "2025-01-15";
      affects = [ "devenv.cli" ];
      description = ''
        **New Feature**: Added changelog system

        Changelogs are now displayed after `devenv update` when the devenv input is updated.
        See the [documentation](https://devenv.sh/changelogs/) for details.
      '';
    }
    {
      date = "2025-01-10";
      affects = [ "languages.rust.enable" "languages.rust.version" ];
      description = ''
        **Breaking Change**: Updated Rust language module options

        Migration guide:
        ```nix
        # Old syntax
        languages.rust.enable = true;

        # New syntax (example)
        languages.rust.enable = true;
        languages.rust.version = "latest";
        ```
      '';
    }
    {
      date = "2024-12-20";
      affects = [ "devenv.root" ];
      description = "Fixed bug in initialization routine";
    }
  ];
}
