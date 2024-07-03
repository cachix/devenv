{ pkgs, ... }: {
  just = {
    enable = true;
    recipes = {
      convco.enable = true;
      hello = {
        enable = true;
        justfile = ''
          # test hello
          hello:
            echo Hello World;
        '';
      };
    };
  };

  scripts.hello-scripts = {
    exec = ''
      echo "Hello Script!"
    '';
    description = "Hello Script";
    just.enable = true;
  };

}
