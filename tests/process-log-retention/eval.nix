{
  lib,
  module ? ../../src/modules/processes.nix,
}:
let
  evaluate =
    global: process:
    (lib.evalModules {
      specialArgs.pkgs = {
        # Keep the real task config defaults without building shell scripts.
        writeScript = _name: _text: "/nix/store/00000000000000000000000000000000-task-script";
      };
      modules = [
        module
        (builtins.dirOf module + "/process-managers/native.nix")
        (builtins.dirOf module + "/tasks.nix")
        (builtins.dirOf module + "/changelogs.nix")
        {
          # Keep the real task submodule schema so undeclared forwarded
          # fields fail evaluation. Only unrelated top-level outputs are
          # unchecked; no packages or scripts need to be built here.
          options.devenv.cli.version = lib.mkOption {
            type = lib.types.nullOr lib.types.str;
            default = "2.4.1";
          };
          config = {
            _module.check = false;
            process.manager.implementation = "native";
            process.managers.native.logs = global;
            tasks.direct = {
              type = "process";
              exec = "true";
              process.logs.maxFiles = 3;
            };
            processes.test = {
              exec = "true";
            }
            // process;
          };
        }
      ];
    }).config;
  standard = evaluate { } { };
  inherited = evaluate {
    maxFileSize = 42;
    maxFiles = 4;
  } { };
  custom = evaluate {
    maxFileSize = 42;
    maxFiles = 4;
  } { logs.maxFiles = 1; };
  sizeOnly = evaluate {
    maxFileSize = 42;
    maxFiles = 4;
  } { logs.maxFileSize = 99; };
in
assert lib.any (
  entry: entry.when && entry.title == "Native process logs now have bounded retention"
) standard.changelogs;
assert
  sizeOnly.processes.test.logs == {
    maxFileSize = 99;
    maxFiles = 4;
  };
assert
  standard.processes.test.logs == {
    maxFileSize = 1048576;
    maxFiles = 2;
  };
assert
  inherited.processes.test.logs == {
    maxFileSize = 42;
    maxFiles = 4;
  };
assert
  custom.processes.test.logs == {
    maxFileSize = 42;
    maxFiles = 1;
  };
assert custom.tasks."devenv:processes:test".process.logs == custom.processes.test.logs;
assert custom.tasks."devenv:processes:test".config.process.logs == custom.processes.test.logs;
assert
  standard.tasks.direct.config.process.logs == {
    maxFileSize = 1048576;
    maxFiles = 3;
  };
assert
  inherited.tasks.direct.config.process.logs == {
    maxFileSize = 42;
    maxFiles = 3;
  };
assert
  !(builtins.tryEval (evaluate { maxFileSize = 0; } { }).processes.test.logs.maxFileSize).success;
assert
  !(builtins.tryEval (evaluate { } { logs.maxFiles = -1; }).processes.test.logs.maxFiles).success;
true
