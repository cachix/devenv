{
  inputs,
  lib,
  pkgs,
  ...
}:
let
  source = pkgs.writeText "copy-source" "new content\n";
  executable = pkgs.runCommand "executable-source" { } ''
    cp ${source} $out
    chmod 555 $out
  '';
  directory = pkgs.runCommand "directory-source" { } ''
    mkdir $out
    cp ${source} $out/child
  '';
  writer =
    mode: file:
    let
      module = import "${inputs.devenv}/files.nix" {
        inherit pkgs lib;
        config = {
          devenv.root = ".";
          devenv.state = ".";
          files.destination = {
            inherit file;
            copyMode = mode;
          };
        };
      };
    in
    pkgs.writeShellScript "files-${mode}" (
      ''
        set -e
      ''
      + module.config.tasks."devenv:files".exec
    );
in
{
  packages = [ pkgs.python3 ];
  env.FILE_COPY_WRITERS = builtins.toJSON {
    copy = writer "copy" source;
    seed = writer "seed" source;
    executable = writer "copy" executable;
    directory = writer "copy" directory;
    seedDirectory = writer "seed" directory;
  };
  env.FILE_COPY_SOURCE = source;
  enterTest = ''
    python ${./test.py}
  '';
}
