{ pkgs, lib, ... }:

lib.mkIf pkgs.stdenv.isDarwin {
  apple.sdk = null;

  # Test that there is no default SDK set on macOS.
  enterTest = ''
    variables_to_check=(
      "DEVELOPER_DIR"
      "DEVELOPER_DIR_FOR_BUILD"
      "SDKROOT"
      "NIX_APPLE_SDK_VERSION"
    )

    for var in "''${variables_to_check[@]}"; do
      if [ -v "$var" ]; then
        echo "$var is set. Expected no default Apple SDK." >&2
        exit 1
      fi
    done
  '';
}
