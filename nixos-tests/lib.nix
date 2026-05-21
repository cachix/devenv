{ pkgs
, lib
, devenv ? null
}:

let
  inherit (lib) filter listToAttrs nameValuePair concatMapStringsSep optional;

  defaultCaps = {
    shell = null;
    framework = null;
    tools = [ ];
    loginShell = false;
    withGit = false;
    withDirenv = false;
    hasProject = false;
  };

  mkFixture =
    { name
    , caps ? { }
    , module
    , project ? null
    }:
    {
      inherit name module project;
      caps = defaultCaps // caps // { hasProject = project != null; };
    };

  mkTest =
    { name
    , requires ? _: true
    , script
    }:
    { inherit name requires script; };

  buildFixture = fixture: tests:
    let
      matching = filter (t: t.requires fixture.caps) tests;
      subtests = concatMapStringsSep "\n\n"
        (t: ''
          with subtest("${t.name}"):
          ${lib.pipe t.script [
            (lib.splitString "\n")
            (map (l: "    " + l))
            (lib.concatStringsSep "\n")
          ]}
        '')
        matching;

      hasProject = fixture.project != null;

      projectConfig = lib.mkIf hasProject {
        devenvTest.project = {
          enable = true;
          inherit (fixture.project) devenvYaml devenvNix;
          devenvLock = fixture.project.devenvLock or null;
          flakeLock = fixture.project.flakeLock or null;
        };
      };
    in
    pkgs.testers.runNixOSTest {
      name = "devenv-${fixture.name}";
      containers.machine = { ... }: lib.mkMerge [
        {
          imports = [
            ./rc-module.nix
            ./project-module.nix
            fixture.module
          ];

          users.users.dev = {
            isNormalUser = true;
            home = "/home/dev";
            createHome = false;
          };

          environment.systemPackages = optional (devenv != null) devenv;
        }
        projectConfig
      ];

      testScript = ''
        machine.start()
        machine.systemctl("start network-online.target")
        machine.wait_for_unit("network-online.target")

        ${subtests}
      '';
    };

  buildMatrix = { fixtures, tests }:
    listToAttrs (map (f: nameValuePair f.name (buildFixture f tests)) fixtures);

in
{
  inherit mkFixture mkTest buildMatrix;
}
