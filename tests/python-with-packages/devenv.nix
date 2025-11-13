{ pkgs, config, ... }:
{
  languages.python = {
    enable = true;

    # Load up some python packages via Nix
    package = pkgs.python3.withPackages (ps: [
      ps.matplotlib
      ps.numpy
      ps.ipython
      ps.tkinter
    ]);

    # Enable the patch (enabled by default)
    # patches.buildEnv.enable = true;

    # Enable the virtual environment
    venv.enable = true;
  };

  profiles.uv-with-packages.module = {
    languages.python = {
      directory = "./uv-profile";
      uv = {
        enable = true;
        sync.enable = true;
      };
    };
  };

  profiles.pip-with-packages.module = {
    languages.python = {
      directory = "./pip-profile";
      venv.requirements = ./pip-profile/requirements.txt;
    };
  };
}
