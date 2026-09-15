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
    proxy.https.enable = true;
    ports.http.allocate = 4321;
    exec = "npm run dev -- --port ${toString config.processes.docs.ports.http.value}";
    cwd = "${config.git.root}/docs";
  };
}
