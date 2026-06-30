# Android

Getting a full working Android development environment is as simple as:

```nix title="devenv.nix"
{ pkgs, ... }:

{
  android.enable = true;
}
```

For a more tailored development environment you can specify specific options.
Note that `platformTools.version` and `emulator.version` default to the latest available versions from nixpkgs:

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
    # platformTools.version defaults to latest from nixpkgs
    buildTools.version = [ "30.0.3" ];
    emulator = {
      enable = true;
      # version defaults to latest from nixpkgs
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

Since Android contains many unfree packages, you need to allow unfree packages in devenv.yaml:

```yaml title="devenv.yaml"
nixpkgs:
  allow_unfree: true
```

## Choosing SDK versions

The platform, build-tool, NDK and other versions you can select with
`platforms.version`, `buildTools.version`, `ndk.version` and friends come from the
`androidenv` packages bundled with nixpkgs. That set lags behind Google's releases,
so a recently released version (for example `platforms;android-36`) may not be
available yet, and selecting it fails.

To pick from the full, up-to-date set of versions, add the
[android-nixpkgs](https://github.com/tadfisher/android-nixpkgs) input, which is
regenerated daily from Google's SDK repositories. Its presence switches the SDK
source, and the release track is chosen by the input's URL ref (`stable` by default,
or `beta`, `preview`, `canary`):

```shell-session
$ devenv inputs add android-nixpkgs github:tadfisher/android-nixpkgs --follows nixpkgs
```

Use a different release track by pointing the URL at `.../beta`, `.../preview` or `.../canary` instead.

The same options now resolve against android-nixpkgs, so you can select newer
versions. `platforms.version`, `buildTools.version`, `ndk.version` and
`emulator.enable` are mapped onto SDK packages automatically. Other components
(system images, sources, CMake, extras) aren't covered by those options; install
them by adding the android-nixpkgs derivation you want to devenv's `packages`
directly:

```nix title="devenv.nix"
{ inputs, pkgs, ... }:

{
  android = {
    enable = true;
    platforms.version = [ "34" "35" "36" ];
    buildTools.version = [ "35.0.0" ];
  };

  packages = [
    (inputs.android-nixpkgs.sdk.${pkgs.system} (sdkPkgs: with sdkPkgs; [
      system-images-android-36-google-apis-playstore-x86-64
    ]))
  ];
}
```

!!! note
    android-nixpkgs supports `x86_64-linux`, `x86_64-darwin` and `aarch64-darwin`.

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
