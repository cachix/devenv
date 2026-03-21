{ pkgs, ... }:
{
  # Test that python packages added directly to `packages` are importable.
  # This is the `packages = [ pkgs.python3Packages.foo ]` pattern,
  # as opposed to `python3.withPackages`.
  packages = [
    pkgs.python3Packages.requests
    pkgs.zlib
  ];

  languages.python = {
    enable = true;
    venv.enable = true;
    venv.requirements = ''
      numpy
    '';
  };

  enterTest = ''
    # Test that packages from `packages = [ pkgs.python3Packages.requests ]` are importable
    python -c "import requests; print('requests version:', requests.__version__)"

    # Test that venv-installed packages are importable
    python -c "import numpy; print('numpy version:', numpy.__version__)"

    # Test that venv packages take priority over profile packages.
    # numpy is installed in the venv, so its path should be under the venv.
    python <<'PYEOF'
    import numpy, os
    venv = os.environ.get("VIRTUAL_ENV", "")
    assert venv, "VIRTUAL_ENV should be set"
    assert venv in numpy.__file__, "numpy should be imported from venv, but got " + numpy.__file__
    print("venv priority OK: numpy loaded from", numpy.__file__)
    PYEOF
  '';
}
