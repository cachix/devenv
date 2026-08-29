{ pkgs, lib, config, ... }:

let
  inherit (builtins) mapAttrs;
  inherit (lib) types optionalAttrs optionalString mkOption attrNames filter length mapAttrsToList concatStringsSep head;

  formats = {
    ini = pkgs.formats.ini { };
    json = pkgs.formats.json { };
    yaml = pkgs.formats.yaml { };
    toml = pkgs.formats.toml { };
    text = {
      type = types.str;
      generate = filename: text: pkgs.writeText filename text;
    };
    source = {
      type = types.path;
      generate = filename: path: path;
    };
  };

  # State tracking for cleanup
  filesStateFile = "${config.devenv.state}/files.json";
  root = lib.removeSuffix "/" (toString config.devenv.root);
  normalizePath =
    path:
    let
      relative =
        if lib.hasPrefix "/" path then
          if lib.hasPrefix "${root}/" path then lib.removePrefix "${root}/" path else null
        else
          path;
      components =
        if relative == null then
          [ ]
        else
          filter (component: component != "" && component != ".") (lib.splitString "/" relative);
    in
    if components == [ ] || builtins.elem ".." components then
      null
    else
      concatStringsSep "/" components;
  originalPaths = attrNames config.files;
  invalidManagedFiles = filter (path: normalizePath path == null) originalPaths;
  validPaths = filter (path: normalizePath path != null) originalPaths;
  normalizedFiles = builtins.listToAttrs (
    map
      (path: {
        name = normalizePath path;
        value = config.files.${path};
      })
      validPaths
  );
  currentManagedFiles = attrNames normalizedFiles;
  filesBuiltinVersion = config.lib._selectCliCapability "task-builtin.files-reconcile" [ 1 ];
  filesTask = config.tasks."devenv:files";

  copyModeType = types.enum [ "symlink" "seed" "copy" ];

  fileSpecModule = { name, config, ... }: {
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
  };

  fileType = types.submodule {
    imports = [ fileSpecModule ];

    options.copyMode = mkOption {
      type = copyModeType;
      default = "symlink";
      description = ''
        How to materialize the file in the project root:

        - `symlink` (default): symlink to the read-only file in the Nix store. Edits are not possible; devenv keeps the link pointed at the current contents.
        - `seed`: copy the file into place once, only if it does not already exist, and make it writable. Existing files are left untouched, so your edits are preserved. Useful for seeding configuration from templates the user then edits.
        - `copy`: copy the file into place as a writable file, overwriting it with fresh contents on every shell entry. Source symlinks are followed. Contents and POSIX modes are preserved (with owner-write enabled); xattrs, ACLs, resource forks, and hard-link topology are not portable guarantees. Useful when a tool must write to the file in place but devenv should remain the source of truth.
      '';
    };
  };

  # The fallback runs on older CLIs too. Keep destination resolution rooted and
  # reject symlinked parents before cleanup, mkdir, chmod, or copy can follow one.
  legacySetupScript = ''
    export PATH=${lib.makeBinPath [ pkgs.coreutils ]}:"$PATH"
    configuredRoot=${lib.escapeShellArg root}
    filesRoot=$(cd -- "$configuredRoot" && pwd -P)
    filesStateFile=${lib.escapeShellArg filesStateFile}
    mkdir -p -- "''${filesStateFile%/*}"

    validateParents() {
      local remaining="$1" parent="$filesRoot" component
      while [[ "$remaining" == */* ]]; do
        component="''${remaining%%/*}"
        remaining="''${remaining#*/}"
        parent="$parent/$component"
        if [ -L "$parent" ] || { [ -e "$parent" ] && [ ! -d "$parent" ]; }; then
          printf 'Unsafe managed file parent: %s\n' "$parent" >&2
          return 1
        fi
      done
    }

    normalizeLegacyPath() {
      local remaining="$1" component
      managedPath=""
      case "$remaining" in
        "$configuredRoot/"*) remaining="''${remaining#"$configuredRoot/"}" ;;
        "$filesRoot/"*) remaining="''${remaining#"$filesRoot/"}" ;;
        /*) return 1 ;;
      esac
      while [ -n "$remaining" ]; do
        component="''${remaining%%/*}"
        if [[ "$remaining" == */* ]]; then
          remaining="''${remaining#*/}"
        else
          remaining=""
        fi
        case "$component" in
          ..) return 1 ;;
          ""|.) continue ;;
        esac
        managedPath="''${managedPath:+$managedPath/}$component"
      done
      [ -n "$managedPath" ]
    }
  '';

  createSymlinkScript = filename: fileOption: ''
    fileName=${lib.escapeShellArg filename}
    filePath="$filesRoot/$fileName"
    sourcePath=${lib.escapeShellArg (toString fileOption.file)}
    validateParents "$fileName"
    if [ -L "$filePath" ]; then
      if [ "$(readlink -- "$filePath")" != "$sourcePath" ]; then
        printf 'Updating %s\n' "$fileName"
        ln -sfn -- "$sourcePath" "$filePath"
      fi
      printf '%s\0' "$fileName" >> "$DEVENV_FILES_CREATED"
    elif [ -e "$filePath" ]; then
      printf 'Conflicting managed file %s\n' "$fileName" >&2
    else
      printf 'Creating %s\n' "$fileName"
      mkdir -p -- "''${filePath%/*}"
      ln -s -- "$sourcePath" "$filePath"
      printf '%s\0' "$fileName" >> "$DEVENV_FILES_CREATED"
    fi
  '';

  createCopyScript = filename: fileOption: ''
    fileName=${lib.escapeShellArg filename}
    filePath="$filesRoot/$fileName"
    sourcePath=${lib.escapeShellArg (toString fileOption.file)}
    validateParents "$fileName"
    # A store link from a previous symlink configuration becomes writable.
    if [ -L "$filePath" ] && [[ "$(readlink -- "$filePath")" == /nix/store/* ]]; then
      rm -- "$filePath"
    fi
    ${optionalString (fileOption.copyMode == "copy") ''
      if [ -e "$filePath" ] || [ -L "$filePath" ]; then
        printf 'Overwriting %s\n' "$fileName"
        rm -rf -- "$filePath"
      fi
    ''}
    if [ -e "$filePath" ] || [ -L "$filePath" ]; then
      printf 'Keeping existing %s\n' "$fileName"
    else
      printf 'Creating %s\n' "$fileName"
      mkdir -p -- "''${filePath%/*}"
      cp -RL -- "$sourcePath" "$filePath"
      chmod -R u+w -- "$filePath"
    fi
    printf '%s\0' "$fileName" >> "$DEVENV_FILES_CREATED"
  '';

  createFileScript =
    filename: fileOption:
    if fileOption.copyMode == "symlink" then
      createSymlinkScript filename fileOption
    else
      createCopyScript filename fileOption;

  cleanupScript = ''
    if [ -f "$filesStateFile" ]; then
      while IFS= read -r -d $'\0' prevFile; do
        if ! normalizeLegacyPath "$prevFile"; then
          printf 'Forgetting unsafe legacy managed path: %s\n' "$prevFile" >&2
          continue
        fi
        ${optionalString (currentManagedFiles != [ ]) ''
          case "$managedPath" in
            ${concatStringsSep "|" (map lib.escapeShellArg currentManagedFiles)}) continue ;;
          esac
        ''}
        # Invalid historic paths are forgotten without touching their targets.
        validateParents "$managedPath" || continue
        filePath="$filesRoot/$managedPath"
        if [ -L "$filePath" ] && [[ "$(readlink -- "$filePath")" == /nix/store/* ]]; then
          printf 'Removing orphaned file: %s\n' "$managedPath"
          rm -- "$filePath"
          parentDir="''${filePath%/*}"
          while [ "$parentDir" != "$filesRoot" ]; do
            rmdir -- "$parentDir" 2>/dev/null || break
            parentDir="''${parentDir%/*}"
          done
        fi
      done < <(${pkgs.jq}/bin/jq -j '.managedFiles[]? | select(type == "string") | ., "\u0000"' "$filesStateFile" 2>/dev/null || true)
    fi
  '';

  saveStateScript = ''
    ${pkgs.jq}/bin/jq -Rs 'split("\u0000") | map(select(length > 0)) | {managedFiles: .}' \
      "$DEVENV_FILES_CREATED" > "$filesStateFile"
    rm -- "$DEVENV_FILES_CREATED"
  '';

  desiredFiles = mapAttrsToList
    (path: file: {
      inherit path;
      source = toString file.file;
      mode = file.copyMode;
    })
    normalizedFiles;
  desiredDigest = builtins.hashString "sha256" (
    builtins.toJSON {
      version = 1;
      root = toString config.devenv.root;
      files = desiredFiles;
    }
  );
  legacyReconcileScript = ''
    ${legacySetupScript}
    ${cleanupScript}
    export DEVENV_FILES_CREATED=$(mktemp)
    ${concatStringsSep "\n\n" (mapAttrsToList createFileScript normalizedFiles)}
    ${saveStateScript}
  '';

in
{
  options.files = mkOption {
    type = types.attrsOf fileType;
    default = { };
    description = ''
      A set of files that will be linked into devenv root.
      Use relative paths within devenv root. Root-contained absolute paths are
      accepted for compatibility, but discouraged because they are not reproducible.
      Parent traversal and symlinked parent directories are not allowed.
    '';
  };

  config = {
    assertions = [
      {
        assertion = invalidManagedFiles == [ ];
        message = ''
          `files` paths must stay within `devenv.root` without parent traversal.
          Invalid paths: ${concatStringsSep ", " invalidManagedFiles}
        '';
      }
      {
        assertion = length currentManagedFiles == length validPaths;
        message = "Multiple `files` paths normalize to the same destination. Use one relative path per file.";
      }
    ];

    warnings = map
      (
        path:
        "files.${builtins.toJSON path} uses an absolute path. Use the reproducible relative path ${builtins.toJSON (normalizePath path)} instead."
      )
      (filter (path: lib.hasPrefix "/" path) validPaths);

    lib.fileSpecType = types.submodule fileSpecModule;
    lib.fileCopyModeType = copyModeType;

    tasks."devenv:files" = { options, ... }: {
      description = "Reconcile managed files";
      exec = legacyReconcileScript;
      # The native runner replaces precisely this generated command. Existing
      # task overrides (including exec = null) must keep their original meaning.
      builtin =
        if
          filesBuiltinVersion == null
          || filesTask.exec != legacyReconcileScript
          || filesTask.package != pkgs.bash
          || filesTask.binary != null
          || filesTask.exports != [ ]
          || filesTask.command == null
          || toString filesTask.command != toString options.command.default
        then
          null
        else
          {
            name = "files-reconcile";
            version = filesBuiltinVersion;
            input = {
              root = toString config.devenv.root;
              state_file = filesStateFile;
              desired_digest = desiredDigest;
              files = desiredFiles;
            };
          };
      before = [ "devenv:enterShell" ];
    };

    infoSections = optionalAttrs (config.files != { }) {
      files = mapAttrsToList (name: file: name) config.files;
    };
  };
}
