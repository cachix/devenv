{ pkgs, config, lib, ... }:

let
  cfg = config.android;

  nixpkgs-android = config.lib.getInput {
    name = "nixpkgs-android";
    url = "github:tadfisher/android-nixpkgs";
    attribute = "android.version";
    follows = [ "nixpkgs" ];
  };

  android-sdk = nixpkgs-android.sdk.${pkgs.stdenv.system} (sdkPkgs: with sdkPkgs; [
    cmdline-tools-latest
    build-tools-34-0-0
    build-tools-33-0-1
    platform-tools
    platforms-android-34
    platforms-android-33
    emulator
    system-images-android-34-google-apis-x86-64
    system-images-android-33-google-apis-x86-64
    ndk-25-2-9519653
  ]);
in
{
  options.android = {
    enable = lib.mkEnableOption "tools for Android Development";

    package = lib.mkOption {
      type = lib.types.package;
      description = "The android packages to use";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = with pkgs; [
      android-sdk
      android-studio
      android-tools
    ];

    env.ANDROID_HOME = "${android-sdk}/share/android-sdk";
    env.ANDROID_SDK_ROOT = "${android-sdk}/share/android-sdk";
    env.ANDROID_NDK_HOME = "${android-sdk}/share/android-sdk/ndk/25.2.9519653";
    # override the aapt2 binary that gradle uses with the patched one from the sdk
    env.GRADLE_OPTS = "-Dorg.gradle.project.android.aapt2FromMavenOverride=${android-sdk}/share/android-sdk/build-tools/34.0.0/aapt2";
    enterShell = ''
          set -e
      	
          export PATH="${android-sdk}/bin:$PATH"
          ${(builtins.readFile "${android-sdk}/nix-support/setup-hook")}
          ANDROID_USER_HOME=$(pwd)/.android
          ANDROID_AVD_HOME=$(pwd)/.android/avd
          export ANDROID_USER_HOME
          export ANDROID_AVD_HOME
          test -e "$ANDROID_USER_HOME" || mkdir -p "$ANDROID_USER_HOME"
          test -e "$ANDROID_AVD_HOME" || mkdir -p "$ANDROID_AVD_HOME"
          set +e
    '';
  };
}
