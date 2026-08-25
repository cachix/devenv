{ pkgs, ... }: {
  packages = [
    (pkgs.writeShellScriptBin "from-flake-import" "true")
  ];
}
