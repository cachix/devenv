{ pkgs, lib, ... }:

let
  types = lib.types;
  ini = pkgs.formats.ini { };
  json = pkgs.formats.json { };
  yaml = pkgs.formats.yaml { };
  toml = pkgs.formats.toml { };
  fileType = types.submodule ({ config, ... }: {
    ini = lib.mkOption {
      type = ini.type;
      default = null;
      description = "TODO";
    };

    json = lib.mkOption {
      type = json.type;
      default = null;
      description = "";
    };
    yaml = lib.mkOption {
      type = yaml.type;
      default = null;
      description = "";
    };
    toml = lib.mkOption {
      type = toml.type;
      default = null;
      description = "";
    };
    text = lib.mkOption {
      type = types.str;
      default = null;
      description = "";
    };
    executable = lib.mkOption {
      type = types.bool;
      default = false;
      description = "";
    };
  });
  createFile = filename: fileOption: ''
    echo "Creating ${filename}"
    mkdir -p $(dirname ${filename})
    ${lib.optionalString fileOption.executable "chmod +x ${filename}"}
  '';
in
{
  options.files = lib.mkOption {
    type = types.attrsOf fileType;
    default = { };
    description = "";
  };

  config = {
    enterShell = lib.mapAttrsToList createFile config.files;
  };
}
