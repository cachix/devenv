{ pkgs, lib, config, ... }:
let
  types = lib.types;

  changelogEntryType = types.submodule {
    options = {
      date = lib.mkOption {
        type = types.strMatching "^[0-9]{4}-[0-9]{2}-[0-9]{2}$";
        description = "Date of the changelog entry in YYYY-MM-DD format.";
        example = "2025-01-15";
      };
      title = lib.mkOption {
        type = types.str;
        description = "Title of the changelog entry.";
        example = "git-hooks.package is now pkgs.prek";
      };
      when = lib.mkOption {
        type = types.bool;
        default = true;
        description = "Whether to include this changelog entry (useful for conditional changelogs).";
      };
      description = lib.mkOption {
        type = types.str;
        description = "Markdown description of the change.";
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

    changelog.json = lib.mkOption {
      type = types.package;
      internal = true;
      description = "The generated changelog.json file containing all changelog entries from all modules.";
    };
  };

  config = {
    changelog.json = (pkgs.formats.json { }).generate "changelog.json" (
      map
        (entry: {
          inherit (entry) date title description;
        })
        (lib.filter (entry: entry.when) config.changelogs)
    );
  };
}
