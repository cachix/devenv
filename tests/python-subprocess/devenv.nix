{ pkgs, ... }:
{
  # Test that Python packages from `packages` are importable in subprocesses.
  # nixpkgs' sitecustomize.py pops NIX_PYTHONPATH from the environment,
  # so without the devenv sitecustomize.py override, child Python processes
  # spawned via subprocess.run() cannot find profile packages.
  packages = [
    pkgs.python3Packages.requests
  ];

  languages.python.enable = true;

  enterTest = ''
    python <<'PYEOF'
    import subprocess, sys

    # Profile packages should be importable in the parent process.
    import requests
    print("parent: requests version:", requests.__version__)

    # Profile packages should also be importable in a subprocess.
    result = subprocess.run(
        [sys.executable, "-c", "import requests; print('child: requests version:', requests.__version__)"],
        capture_output=True, text=True,
    )
    print(result.stdout, end="")
    if result.returncode != 0:
        print(result.stderr, end="")
        raise SystemExit("subprocess failed to import requests")

    print("subprocess import OK")
    PYEOF
  '';
}
