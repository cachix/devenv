{ pkgs, lib, config, ... }:

let
  inherit (builtins) dirOf mapAttrs;
  inherit (lib) types optionalAttrs optionalString mkOption attrNames filter length mapAttrsToList concatStringsSep head assertMsg;
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

  # State tracking for cleanup
  filesStateFile = "${config.devenv.state}/files.json";
  currentManagedFiles = attrNames config.files;


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
        generated = config.format.generate name config.data;
      in
      {
        format = activeFormat;
        data = config.${activeFormatName};
        file =
          if config.executable then
            pkgs.runCommand name { } ''
              cp --no-preserve=mode ${generated} $out
              chmod +x $out
            ''
          else
            generated;
      };
  });
  # Track successfully created files for partial state saving
  createFileScript = filename: fileOption: ''
    if [ -L "${filename}" ]; then
      # Only update symlink if target changed (same content = same store path)
      if [ "$(readlink "${filename}")" != "${fileOption.file}" ]; then
        echo "Updating ${filename}"
        ln -sf ${fileOption.file} "${filename}"
      fi
      echo "${filename}" >> "$DEVENV_FILES_CREATED"
    elif [ -f "${filename}" ]; then
      echo "Conflicting file ${filename}" >&2
    elif [ -e "${filename}" ]; then
      echo "Conflicting non-file ${filename}" >&2
    else
      echo "Creating ${filename}"
      mkdir -p "${dirOf filename}"
      ln -s ${fileOption.file} "${filename}"
      echo "${filename}" >> "$DEVENV_FILES_CREATED"
    fi
  '';

  cleanupScript = ''
    # Read previously managed files from state
    if [ -f '${filesStateFile}' ]; then
      prevFiles=$(${pkgs.jq}/bin/jq -r '.managedFiles[]' '${filesStateFile}' 2>/dev/null || true)
    else
      prevFiles=""
    fi

    # Current files as newline-separated list
    currentFiles='${concatStringsSep "\n" currentManagedFiles}'

    # Find files that were previously managed but are no longer in config
    for prevFile in $prevFiles; do
      # Check if this file is still in current config
      if ! echo "$currentFiles" | grep -qxF "$prevFile"; then
        filePath="${config.devenv.root}/$prevFile"

        # Only remove if it's a symlink pointing to nix store
        if [ -L "$filePath" ]; then
          target=$(readlink "$filePath")
          if [[ "$target" == /nix/store/* ]]; then
            echo "Removing orphaned file: $prevFile"
            rm "$filePath" || echo "Warning: Failed to remove $filePath" >&2

            # Remove empty parent directories up to devenv.root
            parentDir=$(dirname "$filePath")
            while [ "$parentDir" != "${config.devenv.root}" ] && [ -d "$parentDir" ]; do
              if [ -z "$(ls -A "$parentDir")" ]; then
                rmdir "$parentDir" 2>/dev/null || break
                parentDir=$(dirname "$parentDir")
              else
                break
              fi
            done
          fi
        fi
      fi
    done

    ${optionalString (config.files == {}) ''
      # No files configured, save empty state
      echo '{"managedFiles":[]}' > '${filesStateFile}'
    ''}
  '';

  saveStateScript = ''
    # Save successfully created files to state (supports partial success)
    if [ -f "$DEVENV_FILES_CREATED" ]; then
      ${pkgs.jq}/bin/jq -nR '[inputs]' "$DEVENV_FILES_CREATED" | \
        ${pkgs.jq}/bin/jq '{managedFiles: .}' > '${filesStateFile}'
      rm "$DEVENV_FILES_CREATED"
    else
      echo '{"managedFiles":[]}' > '${filesStateFile}'
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
    tasks."devenv:files:cleanup" = {
      description = "Cleanup orphaned files";
      exec = cleanupScript;
      before = [ "devenv:files" "devenv:enterShell" ];
    };

    tasks."devenv:files" = optionalAttrs (config.files != { }) {
      description = "Create files";
      exec = ''
        export DEVENV_FILES_CREATED=$(mktemp)
        ${concatStringsSep "\n\n" (mapAttrsToList createFileScript config.files)}
        ${saveStateScript}
      '';
      after = [ "devenv:files:cleanup" ];
      before = [ "devenv:enterShell" ];
    };

    infoSections = optionalAttrs (config.files != { }) {
      files = mapAttrsToList (name: file: name) config.files;
    };
  };
}
