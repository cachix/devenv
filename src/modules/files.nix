{ pkgs, lib, config, ... }:

let
  inherit (builtins) dirOf mapAttrs;
  inherit (lib) types optionalAttrs mkOption attrNames filter length mapAttrsToList concatStringsSep head assertMsg;
  inherit (types) attrsOf submodule;

  formats = {
    ini = pkgs.formats.ini { };
    json = pkgs.formats.json { };
    yaml = pkgs.formats.yaml { };
    toml = pkgs.formats.toml { };
    text = {
      type = types.str;
      generate = filename: text: pkgs.writeText filename text;
    };
  };


  fileType = types.submodule ({ name, config, ... }: {
    options = {
      format = mkOption {
        type = types.anything;
        default = null;
        internal = true;
        description = "Format of the file";
      };

      data = mkOption {
        type = types.anything;
        default = null;
        internal = true;
        description = "Data of the file";
      };

      file = mkOption {
        type = types.path;
        default = null;
        internal = true;
        description = "Path of the file";
      };

      executable = mkOption {
        type = types.bool;
        default = false;
        description = "Make the file executable";
      };
    } // (mapAttrs
      (name: format: mkOption {
        type = types.nullOr format.type;
        default = null;
        description = "${name} contents";
      })
      formats);

    config =
      let
        formatNames = attrNames formats;
        activeFormatNames = filter (name: config.${name} != null) formatNames;
        activeFormatCount = length activeFormatNames;
        activeFormatName =
          if activeFormatCount == 1 then head activeFormatNames
          else if activeFormatCount > 1 then throw "Multiple formats specified for 'files.${name}'"
          else throw "No contents specified for 'files.${name}'";
        activeFormat = formats.${activeFormatName};
      in
      {
        format = activeFormat;
        data = config.${activeFormatName};
        file = config.format.generate name config.data;
      };
  });
  createFileScript = filename: fileOption: ''
    echo "Creating ${filename}"
    mkdir -p "${dirOf filename}"
    if [ -L "${filename}" ]
    then
      echo "Overwriting ${filename}"
      ln -sf ${fileOption.file} "${filename}"
    elif [ -f "${filename}" ]
    then
      echo "Conflicting file ${filename}" >&2
      exit 1
    else
      echo "Creating ${filename}"
      ln -s ${fileOption.file} "${filename}"
    fi
  '';
in
{
  options.files = mkOption {
    type = types.attrsOf fileType;
    default = { };
    description = "A set of files that will be linked into devenv root.";
  };

  config = {
    tasks."devenv:files" = optionalAttrs (config.files != { }) {
      description = "Create files";
      exec = concatStringsSep "\n\n" (mapAttrsToList createFileScript config.files);
      before = [ "devenv:enterShell" ];
    };

    infoSections = optionalAttrs (config.files != { }) {
      files = mapAttrsToList (name: file: name) config.files;
    };
  };
}
