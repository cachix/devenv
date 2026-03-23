# devenv.yaml

## clean.enabled

Clean the environment when entering the shell.

*Type:* `boolean` · *Default:* `false` · *Added in 1.0*

## clean.keep

A list of environment variables to keep when cleaning the environment.

*Type:* `list of string` · *Default:* `[]` · *Added in 1.0*

## imports

A list of relative paths, absolute paths, or references to inputs to import `devenv.nix` and `devenv.yaml` files.
See [Composing using imports](../composing-using-imports.md).

*Type:* `list of string` · *Default:* `[]`

## impure

Relax the hermeticity of the environment.

*Type:* `boolean` · *Default:* `false` · *Added in 1.0*

## inputs

Map of Nix inputs.
See [Inputs](../inputs.md).

*Type:* `attribute set of input` · *Default:* `inputs.nixpkgs.url: github:cachix/devenv-nixpkgs/rolling`

## inputs.\<name\>.flake

Does the input contain `flake.nix` or `devenv.nix`.

*Type:* `boolean` · *Default:* `true`

## inputs.\<name\>.follows

Another input to "inherit" from by name.
See [Following inputs](../inputs.md#following-inputs).

*Type:* `string`

## inputs.\<name\>.inputs.\<name\>.follows

Override nested inputs by name.
See [Following inputs](../inputs.md#following-inputs).

*Type:* `string`

## inputs.\<name\>.overlays

A list of overlays to include from the input.
See [Overlays](../overlays.md).

*Type:* `list of string` · *Default:* `[]`

## inputs.\<name\>.url

URI specification of the input.
See [Supported URI formats](../inputs.md#supported-uri-formats).

*Type:* `string`

## nixpkgs.androidSdk.acceptLicense

Accept the Android SDK license.
Can also be set via the `NIXPKGS_ACCEPT_ANDROID_SDK_LICENSE=1` environment variable.

*Type:* `boolean` · *Default:* `false`

## nixpkgs.allowBroken

Allow packages marked as broken.

*Type:* `boolean` · *Default:* `false` · *Added in 1.7*

## nixpkgs.allowNonSource

Allow packages not built from source.

*Type:* `boolean` · *Default:* `true` (nixpkgs default)

## nixpkgs.allowUnsupportedSystem

Allow packages that are not supported on the current system.

*Type:* `boolean` · *Default:* `false` · *Added in 2.0.5*

## nixpkgs.allowUnfree

Allow unfree packages.

*Type:* `boolean` · *Default:* `false` · *Added in 1.7*

## nixpkgs.allowlistedLicenses

A list of license names to allow.
Uses nixpkgs license attribute names (e.g. `gpl3Only`, `mit`, `asl20`).
See [nixpkgs license list](https://github.com/NixOS/nixpkgs/blob/master/lib/licenses.nix).

*Type:* `list of string` · *Default:* `[]`

## nixpkgs.blocklistedLicenses

A list of license names to block.
Uses nixpkgs license attribute names (e.g. `unfree`, `bsl11`).
See [nixpkgs license list](https://github.com/NixOS/nixpkgs/blob/master/lib/licenses.nix).

*Type:* `list of string` · *Default:* `[]`

## nixpkgs.cudaCapabilities

Select CUDA capabilities for nixpkgs.

*Type:* `list of string` · *Default:* `[]` · *Added in 1.7*

## nixpkgs.cudaSupport

Enable CUDA support for nixpkgs.

*Type:* `boolean` · *Default:* `false` · *Added in 1.7*

## nixpkgs.rocmSupport

Enable ROCm support for nixpkgs.

*Type:* `boolean` · *Default:* `false` · *Added in 2.0.7*

## nixpkgs.permittedInsecurePackages

A list of insecure permitted packages.

*Type:* `list of string` · *Default:* `[]` · *Added in 1.7*

## nixpkgs.permittedUnfreePackages

A list of unfree packages to allow by name.

*Type:* `list of string` · *Default:* `[]` · *Added in 1.9*

## nixpkgs.per-platform.\<system\>

Per-platform nixpkgs configuration.
Accepts the same options as `nixpkgs`.

*Type:* `attribute set of nixpkgs config` · *Added in 1.7*

## profile

Default profile to activate.
Can be overridden by `--profile` CLI flag.
See [Profiles](../profiles.md).

*Type:* `string` · *Added in 1.11*

## reload

Enable auto-reload of the shell when files change.
Can be overridden by `--reload` or `--no-reload` CLI flags.

*Type:* `boolean` · *Default:* `true` · *Added in 2.0*

## secretspec.enable

Enable [secretspec integration](../integrations/secretspec.md).

*Type:* `boolean` · *Default:* `false` · *Added in 1.8*

## secretspec.profile

Secretspec profile name to use.

*Type:* `string` · *Added in 1.8*

## secretspec.provider

Secretspec provider to use.

*Type:* `string` · *Added in 1.8*

## strictPorts

Error if a port is already in use instead of auto-allocating the next available port.
Can be overridden by `--strict-ports` or `--no-strict-ports` CLI flags.

*Type:* `boolean` · *Default:* `false`
