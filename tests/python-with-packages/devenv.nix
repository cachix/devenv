{ pkgs, ... }:
{
  languages.python = {
    enable = true;
    package = pkgs.python3.withPackages (ps: [
      ps.matplotlib
      ps.numpy
      ps.ipython
      ps.tkinter
    ]);
    patches.buildEnv.enable = true;
    venv.enable = true;
    venv.requirements = ''
      requests
      pytest
    '';
  };

  enterTest = ''
        echo "Verifying sys.base_prefix points to wrapped python..."
        python <<'EOF'
    import sys
    import os

    print("sys.base_prefix:", sys.base_prefix)
    print("sys.executable:", sys.executable)

    # Check that sys.base_prefix points to the -env buildEnv, not the bare interpreter
    assert "-env" in sys.base_prefix, \
        f"sys.base_prefix ({sys.base_prefix}) should point to python-env with packages, not bare interpreter"

    # Verify packages from withPackages are accessible from base_prefix
    site_packages = os.path.join(sys.base_prefix, "lib", f"python{sys.version_info.major}.{sys.version_info.minor}", "site-packages")
    matplotlib_path = os.path.join(site_packages, "matplotlib")
    assert os.path.exists(matplotlib_path), \
        f"matplotlib should exist in base_prefix site-packages at {matplotlib_path}, but it doesn't"

    print("✓ sys.base_prefix correctly points to wrapped python with packages")
    EOF

        echo "Testing imports from Nix's withPackages..."
        python -c 'import matplotlib; print("matplotlib works!")'
        python -c 'import numpy; print("numpy works!")'
        python -c 'import IPython; print("ipython works!")'
        python -c 'import tkinter; print("tkinter works!")'

        echo "Testing imports from venv..."
        python -c 'import requests; print("requests works!")'
        python -c 'import pytest; print("pytest works!")'
  '';
}
