{ config, ... }:

{
  languages.javascript = {
    enable = true;
    directory = "${config.git.root}/docs";
    npm = {
      enable = true;
      install.enable = true;
    };
  };

  processes.docs = {
    exec = "npm run dev";
    cwd = "${config.git.root}/docs";
  };
}
