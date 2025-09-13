{ pkgs
, lib
, config
, ...
}:
let
  types = lib.types;
in
{
  options = {
    profiles = lib.mkOption {
      type = types.submodule {
        freeformType = types.attrsOf (
          types.submodule {
            options = {
              extends = lib.mkOption {
                type = types.listOf types.str;
                description = "List of profile names to extend/inherit from.";
                default = [ ];
                example = [
                  "base"
                  "backend"
                ];
              };
              config = lib.mkOption {
                type = types.deferredModule;
                description = "Configuration to merge when this profile is active.";
                default = { };
              };
            };
          }
        );
        options = {
          hostname = lib.mkOption {
            type = types.attrsOf (
              types.submodule {
                options = {
                  extends = lib.mkOption {
                    type = types.listOf types.str;
                    description = "List of profile names to extend/inherit from.";
                    default = [ ];
                    example = [
                      "base"
                      "backend"
                    ];
                  };
                  config = lib.mkOption {
                    type = types.deferredModule;
                    description = "Configuration to merge when this hostname matches.";
                    default = { };
                  };
                };
              }
            );
            description = "Profile definitions that are automatically activated based on hostname.";
            default = { };
          };
          user = lib.mkOption {
            type = types.attrsOf (
              types.submodule {
                options = {
                  extends = lib.mkOption {
                    type = types.listOf types.str;
                    description = "List of profile names to extend/inherit from.";
                    default = [ ];
                    example = [
                      "base"
                      "backend"
                    ];
                  };
                  config = lib.mkOption {
                    type = types.deferredModule;
                    description = "Configuration to merge when this username matches.";
                    default = { };
                  };
                };
              }
            );
            description = "Profile definitions that are automatically activated based on username.";
            default = { };
          };
        };
      };
      description = "Profile definitions that can be activated manually or automatically.";
      default = { };
      example = lib.literalExpression ''
        {
          # Manual profiles (activated via --profile)
          "base" = {
            config = {
              languages.nix.enable = true;
              packages = [ pkgs.git ];
            };
          };
          "python-3.14" = {
            extends = [ "base" ];
            config = {
              languages.python.version = "3.14";
            };
          };
          "backend" = {
            extends = [ "base" ];
            config = {
              services.postgres.enable = true;
              services.redis.enable = true;
            };
          };
          "fullstack" = {
            extends = [ "backend" "python-3.14" ];
            config = {
              env.FULL_STACK = "true";
            };
          };
          # Automatic hostname-based profiles
          hostname."work-laptop" = {
            extends = [ "backend" ];
            config = {
              env.WORK_ENV = "true";
            };
          };
          # Automatic user-based profiles
          user."alice" = {
            extends = [ "python-3.14" ];
            config = {
              env.USER_ROLE = "developer";
            };
          };
        }
      '';
    };
  };
}
