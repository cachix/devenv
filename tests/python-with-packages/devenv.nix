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
  };

  enterTest = ''
    python -c 'import matplotlib; print("matplotlib works!")'
    python -c 'import numpy; print("numpy works!")'
    python -c 'import IPython; print("ipython works!")'
    python -c 'import tkinter; print("tkinter works!")'
  '';
}
