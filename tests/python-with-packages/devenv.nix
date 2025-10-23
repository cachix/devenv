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
    venv.enable = true;
    venv.requirements = ''
      requests
      pytest
    '';
  };

  enterTest = ''
    echo "Testing imports from Nix's withPackages..."
    python -c 'import matplotlib; print("matplotlib works!")'
    python -c 'import numpy; print("numpy works!")'
    python -c 'import IPython; print("ipython works!")'
    python -c 'import tkinter; print("tkinter works!")'

    echo "Testing imports from venv..."
    python -c 'import requests; print("requests works!")'
    python -c 'import pytest; print("pytest works!")'

    echo "Verifying Nix packages still accessible from venv..."
    python -c 'import matplotlib; print("matplotlib still works!")'
    python -c 'import numpy; print("numpy still works!")'
    python -c 'import IPython; print("ipython still works!")'
    python -c 'import tkinter; print("tkinter still works!")'
  '';
}
