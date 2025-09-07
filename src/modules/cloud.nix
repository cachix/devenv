{ config, lib, ... }:

{
  options.cloud = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Enable cloud-specific configuration";
    };

    ci = {
      github = {
        enabled = lib.mkOption {
          type = lib.types.bool;
          default = false;
          description = "Set to true when running in a GitHub CI environment";
        };

        actor = lib.mkOption {
          type = lib.types.str;
          default = "";
          description = "The username of the person or app that triggered the workflow";
        };

        event = {
          name = lib.mkOption {
            type = lib.types.str;
            default = "";
            description = "The name of the event that triggered the workflow (like push, pull_request, etc.)";
          };
        };

        ref = {
          full = lib.mkOption {
            type = lib.types.str;
            default = "";
            description = "The full Git ref that triggered the run (e.g. refs/heads/main for a push)";
          };

          name = lib.mkOption {
            type = lib.types.str;
            default = "";
            description = "A shorter version of the ref (e.g., just main or the tag name)";
          };
        };

        repository = lib.mkOption {
          type = lib.types.str;
          default = "";
          description = "The owner and repository name (e.g. octocat/Hello-World)";
        };

        sha = lib.mkOption {
          type = lib.types.str;
          default = "";
          description = "The commit SHA that triggered the workflow";
        };

        job = {
          id = lib.mkOption {
            type = lib.types.str;
            default = "";
            description = "Unique identifier for the workflow run";
          };
        };

      };
    };
  };
}
