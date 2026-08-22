{ pkgs, lib, ... }:

{
  languages.javascript = {
    enable = true;
    npm.install.enable = true;
  };

  processes.nodejs.exec = "node app.js";

}
