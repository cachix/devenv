# Android

Getting a full working Android development environment is as simple as:

```nix title="devenv.nix"
{ pkgs, ... }:

{
  android.enable = true;
}
```

For a more tailored development environment you can specify specific options:

```nix title="devenv.nix"
{ pkgs, ... }:

{
  android = {
    enable = true;
    platforms.version = [ "32" "34" ];
    systemImageTypes = [ "google_apis_playstore" ];
    abis = [ "arm64-v8a" "x86_64" ];
    cmake.version = [ "3.22.1" ];
    cmdLineTools.version = "11.0";
    tools.version = "26.1.1";
    platformTools.version = "34.0.5";
    buildTools.version = [ "30.0.3" ];
    emulator = {
      enable = true;
      version = "34.1.9";
    };
    sources.enable = false;
    systemImages.enable = true;
    ndk.enable = true;
    googleAPIs.enable = true;
    googleTVAddOns.enable = true;
    extras = [ "extras;google;gcm" ];
    extraLicenses = [
      "android-sdk-preview-license"
      "android-googletv-license"
      "android-sdk-arm-dbt-license"
      "google-gdk-license"
      "intel-android-extra-license"
      "intel-android-sysimage-license"
      "mips-android-sysimage-license"
    ];
    android-studio = {
      enable = true;
      package = pkgs.android-studio;
    };
  };
}
```

Since Android contains many unfree packages, you need to set allowUnfree: true in devenv.yaml:

```nix title="devenv.yaml"
allowUnfree: true
```

## Emulators

Creating emulators via the android-studio GUI may not work as expected due to conflicts between the immutable Nix store paths and Android Studio requiring a mutable path. Therefore, it's recommended to create an emulator via the CLI:

### Creating an emulator
```nix title="bash"
avdmanager create avd --force --name my-android-emulator-name --package 'system-images;android-32;google_apis_playstore;x86_64'
```

After creating the emulator, you can use any text editor to develop for Android. During testing, we successfully ran a React Native project inside Android Studio by first creating the Android emulator externally as described above and then running the project inside the android-studio's terminal.

## React Native
The following config works with react native starter project.
```nix title="devenv.nix"
{ pkgs, ... }:

{
  android = {
    enable = true;
    reactNative.enable = true;
  };
}
```

## Flutter

The following config works with the flutter starter project.

```nix title="devenv.nix"
{ pkgs, ... }:

{
  android = {
    enable = true;
    flutter.enable = true;
  };
}
```
