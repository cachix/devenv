{ pkgs
, lib
, config
, ...
}:
let
  types = lib.types;

  profileModule = {
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
      module = lib.mkOption {
        type = types.deferredModule;
        description = "Additional configuration to merge when this profile is active.";
        default = { };
      };
    };
  };

  profileType = types.lazyAttrsOf (types.submodule profileModule);
in
{
  options = {
    profiles = lib.mkOption {
      type = types.submodule {
        freeformType = profileType;
        options = {
          hostname = lib.mkOption {
            type = profileType;
            description = "Profile definitions that are automatically activated based on the machine's hostname.";
            default = { };
          };
          user = lib.mkOption {
            type = profileType;
            description = "Profile definitions that are automatically activated based on the username.";
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
            module = {
              languages.nix.enable = true;
              packages = [ pkgs.git ];
            };
          };
          "python-3.14" = {
            extends = [ "base" ];
            module = {
              languages.python.version = "3.14";
            };
          };
          "backend" = {
            extends = [ "base" ];
            module = {
              services.postgres.enable = true;
              services.redis.enable = true;
            };
          };
          "fullstack" = {
            extends = [ "backend" "python-3.14" ];
            module = {
              env.FULL_STACK = "true";
            };
          };
          # Automatic hostname-based profiles
          hostname."work-laptop" = {
            extends = [ "backend" ];
            module = {
              env.WORK_ENV = "true";
            };
          };
          # Automatic user-based profiles
          user."alice" = {
            extends = [ "python-3.14" ];
            module = {
              env.USER_ROLE = "developer";
            };
          };
        }
      '';
    };
  };
}
