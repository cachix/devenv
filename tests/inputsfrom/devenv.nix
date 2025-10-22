{ pkgs, ... }:
let
  # Create a simple derivation with some build inputs
  myDerivation = pkgs.stdenv.mkDerivation {
    name = "test-derivation";
    buildInputs = [ pkgs.hello pkgs.cowsay ];
    nativeBuildInputs = [ pkgs.jq ];
    dontUnpack = true;
    installPhase = "mkdir -p $out";
  };
in
{
  # Use inputsFrom to inherit the build inputs from myDerivation
  inputsFrom = [ myDerivation ];

  enterTest = ''
    # Test that hello is available (from buildInputs)
    if ! command -v hello &> /dev/null; then
      echo "ERROR: hello command not found (should be inherited from inputsFrom)"
      exit 1
    fi

    # Test that cowsay is available (from buildInputs)
    if ! command -v cowsay &> /dev/null; then
      echo "ERROR: cowsay command not found (should be inherited from inputsFrom)"
      exit 1
    fi

    # Test that jq is available (from nativeBuildInputs)
    if ! command -v jq &> /dev/null; then
      echo "ERROR: jq command not found (should be inherited from inputsFrom)"
      exit 1
    fi

    echo "All inputsFrom packages are available!"
  '';
}
