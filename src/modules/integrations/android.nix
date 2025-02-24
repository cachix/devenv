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
    buildToolsVersions = cfg.buildTools.version;
    includeEmulator = cfg.emulator.enable;
    emulatorVersion = cfg.emulator.version;
    platformVersions = cfg.platforms.version;
    includeSources = cfg.sources.enable;
    includeSystemImages = cfg.systemImages.enable;
    systemImageTypes = cfg.systemImageTypes;
    abiVersions = cfg.abis;
    cmakeVersions = cfg.cmake.version;
    includeNDK = cfg.ndk.enable;
    ndkVersions = cfg.ndk.version;
    useGoogleAPIs = cfg.googleAPIs.enable;
    useGoogleTVAddOns = cfg.googleTVAddOns.enable;
    includeExtras = cfg.extras;
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

    platforms.version = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "32" "34" ];
      description = ''
        The Android platform versions to install.
        By default, versions 32 and 34 are installed.
      '';
    };

    systemImageTypes = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "google_apis_playstore" ];
      description = ''
        The Android system image types to install.
        By default, the google_apis_playstore system image is installed.
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

    cmake.version = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "3.22.1" ];
      description = ''
        The CMake versions to install for Android.
        By default, version 3.22.1 is installed.
      '';
    };

    cmdLineTools.version = lib.mkOption {
      type = lib.types.str;
      default = if cfg.flutter.enable then "8.0" else "11.0";
      description = ''
        The version of the Android command line tools to install.
        By default, version 11.0 is installed or 8.0 if flutter is enabled.
      '';
    };

    tools.version = lib.mkOption {
      type = lib.types.str;
      default = "26.1.1";
      description = ''
        The version of the Android SDK tools to install.
        By default, version 26.1.1 is installed.
      '';
    };

    platformTools.version = lib.mkOption {
      type = lib.types.str;
      default = if cfg.flutter.enable then "34.0.4" else "34.0.5";
      description = ''
        The version of the Android platform tools to install.
        By default, version 34.0.5 is installed or 34.0.5 if flutter is enabled.
      '';
    };

    buildTools.version = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = if cfg.flutter.enable then [ "33.0.2" "30.0.3" ] else [ "34.0.0" ];
      description = ''
        The version of the Android build tools to install.
        By default, version 30.0.3 is installed or [ "33.0.2" "30.0.3" ] if flutter is enabled.
      '';
    };

    emulator.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Whether to include the Android Emulator.
        By default, the emulator is included.
      '';
    };

    emulator.version = lib.mkOption {
      type = lib.types.str;
      default = "34.1.9";
      description = ''
        The version of the Android Emulator to install.
        By default, version 34.1.9 is installed.
      '';
    };

    sources.enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Whether to include the Android sources.
        By default, the sources are not included.
      '';
    };

    systemImages.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Whether to include the Android system images.
        By default, the system images are included.
      '';
    };

    ndk.enable = lib.mkOption {
      type = lib.types.bool;
      default = !cfg.flutter.enable;
      description = ''
        Whether to include the Android NDK (Native Development Kit).
        By default, the NDK is included.
      '';
    };

    ndk.version = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "26.1.10909125" ];
      description = ''
        The version of the Android NDK (Native Development Kit) to install.
        By default, version 26.1.10909125 is installed.
      '';
    };

    googleAPIs.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Whether to use the Google APIs.
        By default, the Google APIs are used.
      '';
    };

    googleTVAddOns.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Whether to use the Google TV Add-Ons.
        By default, the Google TV Add-Ons are used.
      '';
    };

    extras = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "extras;google;gcm" ];
      description = ''
        The Android extras to install.
        By default, the Google Cloud Messaging (GCM) extra is installed.
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
        The additional Android licenses to accept.
        By default, several standard licenses are accepted.
      '';
    };

    android-studio.enable = lib.mkEnableOption "the installation of Android Studio";

    android-studio.package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.android-studio;
      defaultText = lib.literalExpression "pkgs.android-studio";
      description = ''
        The Android Studio package to use.
        By default, the Android Studio package from nixpkgs is used.
      '';
    };

    flutter.enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Whether to include the Flutter tools.
      '';
    };

    flutter.package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.flutter;
      defaultText = lib.literalExpression "pkgs.flutter";
      description = ''
        The Flutter package to use.
        By default, the Flutter package from nixpkgs is used.
      '';
    };

    reactNative.enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Whether to include the React Native tools.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    packages = [
      androidSdk
      platformTools
      androidEmulator
    ]
    ++ lib.optional cfg.flutter.enable cfg.flutter.package
    ++ lib.optional cfg.android-studio.enable cfg.android-studio.package;

    # Nested conditional for flutter
    languages = lib.mkMerge [
      { java.enable = true; }
      (lib.mkIf cfg.flutter.enable {
        dart.enable = true;
        # By default, Flutter uses the JDK version that ships Android Studio.
        # Sync with https://developer.android.com/build/jdks
        java.jdk.package = pkgs.jdk17;
      })
      (lib.mkIf cfg.reactNative.enable {
        javascript.enable = true;
        javascript.npm.enable = true;
        # Sync with https://reactnative.dev/docs/set-up-your-environment
        java.jdk.package = pkgs.jdk17;
      })
    ];

    env.ANDROID_HOME = "${androidSdk}/libexec/android-sdk";
    env.ANDROID_NDK_ROOT = "${config.env.ANDROID_HOME}/ndk/";

    # override the aapt2 binary that gradle uses with the patched one from the sdk
    env.GRADLE_OPTS = "-Dorg.gradle.project.android.aapt2FromMavenOverride=${androidSdk}/libexec/android-sdk/build-tools/${lib.head cfg.buildTools.version}/aapt2";

    env.FLUTTER_ROOT = if cfg.flutter.enable then cfg.flutter.package else "";
    env.DART_ROOT = if cfg.flutter.enable then "${cfg.flutter.package}/bin/cache/dart-sdk" else "";

    enterShell = ''
      set -e
      export LD_LIBRARY_PATH="$LD_LIBRARY_PATH:${pkgs.lib.makeLibraryPath [pkgs.vulkan-loader pkgs.libGL]}:${config.env.ANDROID_HOME}/build-tools/${lib.head cfg.buildTools.version}/lib64/:${config.env.ANDROID_NDK_ROOT}/${lib.head cfg.ndk.version}/toolchains/llvm/prebuilt/linux-x86_64/lib/:$LD_LIBRARY_PATH"

      export PATH="$PATH:${config.env.ANDROID_HOME}/tools:${config.env.ANDROID_HOME}/tools/bin:${config.env.ANDROID_HOME}/platform-tools"
      cat <<EOF > local.properties
      # This file was automatically generated by nix-shell.
      sdk.dir=$ANDROID_HOME
      ndk.dir=$ANDROID_NDK_ROOT
      EOF

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
