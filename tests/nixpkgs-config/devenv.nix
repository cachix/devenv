{ pkgs, ... }: {
  env = {
    ALLOW_UNFREE = pkgs.lib.boolToString (pkgs.config.allowUnfree or false);
    CUDA_SUPPORT = pkgs.lib.boolToString (pkgs.config.cudaSupport or false);
    CUDA_CAPABILITIES = builtins.toString (pkgs.config.cudaCapabilities or [ ]);
  };

  enterTest = ''
    if [ -z "$DEVENV_NIX" ]; then
      echo "DEVENV_NIX is not set"
      exit 1
    fi

    if [[ "$ALLOW_UNFREE" != "true" ]]; then
      echo "ALLOW_UNFREE ($ALLOW_UNFREE) != true"
      exit 1
    fi
    if [[ "$CUDA_SUPPORT" != "true" ]]; then
      echo "CUDA_SUPPORT ($CUDA_SUPPORT) != true"
      exit 1
    fi
    if [[ "$CUDA_CAPABILITIES" != "8.0" ]]; then
      echo "CUDA_CAPABILITIES ($CUDA_CAPABILITIES) != 8.0"
      exit 1
    fi
  '';
}
