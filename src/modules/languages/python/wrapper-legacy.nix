{ lib
, stdenv
, buildEnv
, runCommand
, makeBinaryWrapper
, callPackage
, # manually passed
  python
, requiredPythonModules
, # extra opts
  extraLibs ? [ ]
, extraOutputsToInstall ? [ ]
, postBuild ? ""
, ignoreCollisions ? false
, permitUserSite ? false
, # Wrap executables with the given argument.
  makeWrapperArgs ? [ ]
,
}:

# Create a python executable that knows about additional packages.
let
  makePostBuildWrapper = callPackage ./postbuild-wrapper.nix { };

  env =
    let
      paths = requiredPythonModules (extraLibs ++ [ python ]) ++ [
        (runCommand "bin" { } ''
          mkdir -p $out/bin
        '')
      ];
    in
    buildEnv {
      name = "${python.name}-env";

      inherit paths;
      inherit ignoreCollisions;
      extraOutputsToInstall = [ "out" ] ++ extraOutputsToInstall;

      nativeBuildInputs = [ makeBinaryWrapper ];

      postBuild =
        makePostBuildWrapper
          {
            inherit
              python
              permitUserSite
              makeWrapperArgs
              ;
          }
        + postBuild;

      inherit (python) meta;

      passthru = python.passthru // {
        interpreter = "${env}/bin/${python.executable}";
        inherit python;
        env = stdenv.mkDerivation {
          name = "interactive-${python.name}-environment";
          nativeBuildInputs = [ env ];

          buildCommand = ''
            echo >&2 ""
            echo >&2 "*** Python 'env' attributes are intended for interactive nix-shell sessions, not for building! ***"
            echo >&2 ""
            exit 1
          '';
        };
      };
    };
in
env
