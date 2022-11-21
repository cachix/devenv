{ pkgs, config, lib, ... }:

let
  cfg = config.languages.php;
in
{
  options.languages.php = {
    enable = lib.mkEnableOption "Enable tools for PHP development.";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.php;
      defaultText = "pkgs.php";
      description = lib.mdDoc ''
        Allows to [override the default used package](https://nixos.org/manual/nixpkgs/stable/#ssec-php-user-guide) to adjust the settings or add more extensions. You can find the extensions using `devenv search 'php extensions'`
        ```
      '';
      example = lib.literalExpression ''
        pkgs.php.buildEnv {
          extensions = { all, enabled }: with all; enabled ++ [ xdebug ];
          extraConfig = '''
            memory_limit=1G
          ''';
        };
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      cfg.package
      cfg.package.packages.composer
    ];

    enterShell = ''
      php --version
      composer --version
    '';
  };
}
