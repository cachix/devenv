{ pkgs, lib, ... }:

lib.mkIf pkgs.stdenv.isDarwin {
  apple.sdk = pkgs.apple-sdk;

  packages = [
    pkgs.xcbuild
  ];

  # Test that the above SDK is picked up by xcode-select.
  enterTest = ''
    if [ -v "$DEVELOPER_DIR" ]; then
      echo "DEVELOPER_DIR is not set."
      exit 1
    fi

    xcode-select -p | grep -q /nix/store
  '';
}
