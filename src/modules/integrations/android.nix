{ pkgs, config, lib, ... }:

let
  cfg = config.android;

  androidEnv = pkgs.callPackage "${pkgs.path}/pkgs/development/mobile/androidenv" {
    inherit config pkgs;
    licenseAccepted = true;
  };

  sdkArgs = {
    cmdLineToolsVersion = cfg.cmdLineTools.version;
    toolsVersion = cfg.tools.version;
    platformToolsVersion = cfg.platformTools.version;

    # setting the build tools version throws error 
    # buildToolsVersions = [ "30.0.3" ];
    includeEmulator = cfg.includeEmulator.enable;
    emulatorVersion = cfg.emulator.version;
    platformVersions = cfg.platforms.version;
    includeSources = cfg.includeSources.enable;
    includeSystemImages = cfg.includeSystemImages.enable;
    systemImageTypes = cfg.systemImageTypes;
    abiVersions = cfg.abis;
    cmakeVersions = cfg.cmakeVersions;
    includeNDK = cfg.includeNDK.enable;
    useGoogleAPIs = cfg.useGoogleAPIs.enable;
    useGoogleTVAddOns = cfg.useGoogleTVAddOns.enable;
    includeExtras = cfg.includeExtras;

    # Accepting more licenses declaratively:
    extraLicenses = cfg.extraLicenses;
  };

  androidComposition = androidEnv.composeAndroidPackages sdkArgs;
  androidEmulator = androidEnv.emulateApp {
    name = "android-sdk-emulator";
    sdkExtraArgs = sdkArgs;
  };

  androidSdk = androidComposition.androidsdk;
  platformTools = androidComposition.platform-tools;
in
{
  options.android = {
    enable = lib.mkEnableOption "tools for Android Development";

    package = lib.mkOption {
      type = lib.types.package;
      description = "The android packages to use";
    };
  };

    platforms.version = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "34" ];
      description = ''
        The Android platform versions to install.

        By default, the version 34 is installed.
      '';
    };

    systemImageTypes = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "google_apis" ];
      description = ''
        The Android system image types to install.

        By default, the google_apis system image is installed.
      '';
    };

    abis = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "arm64-v8a" "x86_64" ];
      description = ''
        The Android ABIs to install.

        By default, the arm64-v8a and x86_64 ABIs are installed.
      '';
    };

    cmakeVersions = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "3.22.1" ];
      description = ''
        The Android CMake versions to install.

        By default, the version 3.22.1 is installed.
      '';
    };

    cmdLineTools.version = lib.mkOption {
      type = lib.types.str;
      default = "8.0";
      description = ''
        The version of the Android command line tools to install.

        By default, the version 8.0 is installed.
      '';
    };

    tools.version = lib.mkOption {
      type = lib.types.str;
      default = "26.1.1";
      description = ''
        The version of the Android command line tools to install.

        By default, the version 8.0 is installed.
      '';
    };

    platformTools.version = lib.mkOption {
      type = lib.types.str;
      default = "34.0.5";
      description = ''
        The version of the Android command line tools to install.

        By default, the version 8.0 is installed.
      '';
    };

    includeEmulator.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Whether to include the Android Emulator.

        By default, the emulator is included.
      '';
    };

    emulator.version = lib.mkOption {
      type = lib.types.str;
      default = "33.1.6";
      description = ''
        The version of the Android Emulator to install.

        By default, the version 33.1.6 is installed.
      '';
    };

    includeSources.enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Whether to include the Android sources.

        By default, the sources are not included.
      '';
    };

    includeSystemImages.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Whether to include the Android system images.

        By default, the system images are included.
      '';
    };

    includeNDK.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Whether to include the Android NDK.

        By default, the NDK is included.
      '';
    };

    useGoogleAPIs.enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Whether to use the Google APIs.

        By default, the Google APIs are not used.
      '';
    };

    useGoogleTVAddOns.enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Whether to use the Google TV Add-Ons.

        By default, the Google TV Add-Ons are not used.
      '';
    };

    includeExtras = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "extras;google;gcm" ];
      description = ''
        The Android extras to install.

        By default, no extras are installed.
      '';
    };

    extraLicenses = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [
        "android-sdk-preview-license"
        "android-googletv-license"
        "android-sdk-arm-dbt-license"
        "google-gdk-license"
        "intel-android-extra-license"
        "intel-android-sysimage-license"
        "mips-android-sysimage-license"
      ];
      description = ''
        The Android extra licenses to accept.

        By default, the android-sdk-preview-license, android-googletv-license, android-sdk-arm-dbt-license, google-gdk-license, intel-android-extra-license, intel-android-sysimage-license, and mips-android-sysimage-license are accepted.
      '';
    };

    jdk = lib.mkOption {
      type = lib.types.package;
      default = pkgs.jdk;
      defaultText = "pkgs.jdk";
      description = ''
        The jdk package to use.
      '';
      example = "pkgs.jdk";
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      androidSdk
      platformTools
      androidEmulator
      cfg.jdk
      pkgs.android-studio
      pkgs.glibc
    ];

    env.ANDROID_HOME = "${androidSdk}/libexec/android-sdk";
    env.ANDROID_NDK_ROOT = "${config.env.ANDROID_HOME}/ndk-bundle";
    env.GRADLE_OPTS = "-Dorg.gradle.project.android.aapt2FromMavenOverride=${androidSdk}/libexec/android-sdk/build-tools/34.0.0/aapt2";
    # override the aapt2 binary that gradle uses with the patched one from the sdk
    enterShell = ''
      set -v
      export PATH $PATH:${config.env.ANDROID_HOME}/tools:${config.env.ANDROID_HOME}/tools/bin:${config.env.ANDROID_HOME}/platform-tools
      # Write out local.properties for Android Studio.
      cat <<EOF > local.properties
      # This file was automatically generated by nix-shell.
      sdk.dir=$ANDROID_HOME
      ndk.dir=$ANDROID_NDK_ROOT
      ANDROID_USER_HOME=$(pwd)/.android
      ANDROID_AVD_HOME=$(pwd)/.android/avd
      EOF
      set +v
    '';
  };
}
