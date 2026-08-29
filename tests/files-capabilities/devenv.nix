{
  lib,
  ...
}:
let
  filesTask = config: config.tasks."devenv:files";
  hasFallback = config: (filesTask config).command != null;
in
{
  files."managed.txt".text = "managed by devenv\n";

  profiles.absent.module =
    { config, ... }:
    {
      devenv.cli.capabilities = lib.mkForce { };
      assertions = [
        {
          assertion = (filesTask config).builtin == null && hasFallback config;
          message = "files task must omit its builtin when CLI capabilities are absent";
        }
      ];
    };

  profiles.non-overlap.module =
    { config, ... }:
    {
      devenv.cli.capabilities = lib.mkForce {
        "task-builtin.files-reconcile".versions = [ 2 ];
      };
      assertions = [
        {
          assertion = (filesTask config).builtin == null && hasFallback config;
          message = "files task must omit its builtin when protocol versions do not overlap";
        }
      ];
    };

  profiles.overlap.module =
    { config, ... }:
    {
      devenv.cli.capabilities = lib.mkForce {
        "task-builtin.files-reconcile".versions = [
          2
          1
        ];
      };
      assertions = [
        {
          assertion =
            (filesTask config).builtin.name == "files-reconcile"
            && (filesTask config).builtin.version == 1
            && map (file: file.path) (filesTask config).builtin.input.files == [ "managed.txt" ]
            && hasFallback config;
          message = "files task must emit builtin v1 with project-relative paths and retain its command fallback on overlap";
        }
      ];
    };

  profiles.absolute-path.module =
    { config, ... }:
    {
      devenv.cli.capabilities = lib.mkForce {
        "task-builtin.files-reconcile".versions = [ 1 ];
      };
      files."${config.devenv.root}/absolute.txt".text = "accepted with a warning\n";
      files."./nested//value".text = "normalized\n";
      assertions = [
        {
          assertion =
            map (file: file.path) (filesTask config).builtin.input.files == [
              "absolute.txt"
              "managed.txt"
              "nested/value"
            ];
          message = "absolute and equivalent relative paths must normalize within the project root";
        }
      ];
    };

  profiles.override.module =
    { config, ... }:
    {
      devenv.cli.capabilities = lib.mkForce {
        "task-builtin.files-reconcile".versions = [ 1 ];
      };
      tasks."devenv:files".exec = lib.mkForce "touch override-ran";
      assertions = [
        {
          assertion = (filesTask config).builtin == null;
          message = "overriding exec must disable automatic native dispatch";
        }
      ];
    };

  profiles.disabled.module =
    { config, ... }:
    {
      devenv.cli.capabilities = lib.mkForce {
        "task-builtin.files-reconcile".versions = [ 1 ];
      };
      tasks."devenv:files".exec = lib.mkForce null;
      assertions = [
        {
          assertion = (filesTask config).builtin == null && (filesTask config).command == null;
          message = "disabling exec must leave a valid task without a builtin or command";
        }
      ];
    };

  profiles.command-override.module =
    { config, pkgs, ... }:
    {
      devenv.cli.capabilities = lib.mkForce {
        "task-builtin.files-reconcile".versions = [ 1 ];
      };
      tasks."devenv:files".command = lib.mkForce (pkgs.writeScript "files-override" "exit 0");
      assertions = [
        {
          assertion = (filesTask config).builtin == null;
          message = "overriding command must disable automatic native dispatch";
        }
      ];
    };

  profiles.symlink-parent.module = {
    devenv.cli.capabilities = lib.mkForce { };
    files."escape/sentinel" = {
      text = "must not overwrite the external file\n";
      copyMode = "copy";
    };
  };

  profiles.duplicate-path.module = {
    files."./managed.txt".text = "duplicates managed.txt\n";
  };

  profiles.parent-path.module = {
    files."../outside.txt".text = "must be rejected\n";
  };

  profiles.outside-path.module = {
    files."/outside.txt".text = "must be rejected\n";
  };
}
