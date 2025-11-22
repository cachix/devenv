{ pkgs, lib, config, ... }:
let
  types = lib.types;

  changelogEntryType = types.submodule {
    options = {
      date = lib.mkOption {
        type = types.str;
        description = "Date of the changelog entry in YYYY-MM-DD format.";
      };
      description = lib.mkOption {
        type = types.str;
        description = "Markdown description of the change.";
      };
      affects = lib.mkOption {
        type = types.listOf types.str;
        description = "List of configuration attributes affected by this change (e.g., [\"languages.rust.enable\" \"packages\"]).";
      };
    };
  };
in
{
  options = {
    changelogs = lib.mkOption {
      type = types.listOf changelogEntryType;
      default = [ ];
      description = "List of changelog entries for this module.";
    };

    changelog.data = lib.mkOption {
      type = types.package;
      internal = true;
      description = "The generated changelogs.json file containing all changelog entries from all modules.";
    };
  };

  config = {
    changelog.data = (pkgs.formats.json { }).generate "changelogs.json" config.changelogs;
  };
}
