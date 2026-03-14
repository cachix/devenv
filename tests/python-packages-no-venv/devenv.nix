{ pkgs, ... }:
{
  # Test that python packages added directly to `packages` are importable
  # without a venv.
  packages = [
    pkgs.python3Packages.requests
  ];

  languages.python.enable = true;

  enterTest = ''
    python -c "import requests; print('requests version:', requests.__version__)"
  '';
}
