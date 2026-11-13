{
  nixpkgs ? <nixpkgs>,
  envLibrary ? ../../src/modules/lib/env.nix,
}:
let
  lib = import (nixpkgs + "/lib");
  env = import envLibrary { inherit lib; };
  evaluateModule =
    modules:
    (lib.evalModules {
      modules = [
        {
          options.env = lib.mkOption {
            type = lib.types.submodule {
              freeformType = lib.types.lazyAttrsOf env.type;
            };
            apply = env.values;
            default = { };
          };
        }
      ]
      ++ modules;
    });
  evaluate = modules: (evaluateModule modules).config.env;
  definitions =
    modules:
    let
      option = (evaluateModule modules).options.env;
    in
    option.type.merge [ "env" ] option.definitionsWithLocations;
  resolve = modules: env.resolve (definitions modules);
  expectFailure = value: !(builtins.tryEval (builtins.deepSeq value true)).success;
  merged = resolve [
    {
      env = {
        LEGACY = "unchanged";
        BOTH = env.combine [
          (env.beforeSetup "initial")
          (env.set "final")
        ];
        PATHS = env.prepend ":" [ "first" ];
        FORCED = env.append ":" [ "discarded" ];
        PRIORITY = lib.mkDefault (env.set "default");
      };
    }
    {
      env = {
        PATHS = lib.mkAfter (env.append ":" [ "last" ]);
        FORCED = lib.mkForce (env.set "forced");
        PRIORITY = env.set "explicit";
      };
    }
    { env.PATHS = lib.mkBefore (env.prepend ":" [ "earliest" ]); }
    { env.PATHS = env.prepend ":" [ "second" ]; }
  ];
in
{
  checks = {
    legacyUnchanged = (evaluate [ { env.X = "legacy"; } ]).X == "legacy";
    plainEnvironmentAvoidsWrapper =
      env.wrapShell {
        pkgs = { };
        environment.X = "plain";
        shell = "unchanged";
      } == "unchanged";
    configuredSetIsPlain = (evaluate [ { env.X = env.set "plain"; } ]).X == "plain";
    configuredExtensionsArePlain =
      (evaluate [
        {
          env.X = env.combine [
            (env.set "base")
            (env.prepend ":" [ "prefix" ])
          ];
        }
      ]).X == "prefix:base";
    configuredValuesCanBeReferenced =
      (evaluate [
        ({ config, ... }: {
          env.ROOT = env.set "/configured";
          env.CHILD = config.env.ROOT + "/child";
        })
      ]).CHILD == "/configured/child";
    configuredValuesCanBeSerialized =
      (builtins.fromJSON (
        builtins.toJSON (evaluate [
          {
            env.X = env.append ":" [ "suffix" ];
          }
        ])
      )).X == "suffix";
    legacyListsConflict = expectFailure (evaluate [
      { env.X = [ "a" ]; }
      { env.X = [ "b" ]; }
    ]);
    legacyAndFinalCompose =
      (resolve [
        { env.X = "initial"; }
        { env.X = env.set "final"; }
      ]).initial.X == "initial";
    bothPhases = merged.initial.BOTH == "initial";
    finalOnlyNotSeeded = !(merged.initial ? PATHS);
    forcedOperationsDiscarded =
      merged.final.FORCED.append == [ ] && merged.final.FORCED.replacement.value == "forced";
    defaultPriorityOverridden = merged.final.PRIORITY.replacement.value == "explicit";
    orderedPrepends =
      merged.final.PATHS.prepend == [
        "earliest"
        "first"
        "second"
      ];
    appendRetained = merged.final.PATHS.append == [ "last" ];
    conflictingSetsFail = expectFailure (resolve [
      { env.X = env.set "a"; }
      { env.X = env.set "b"; }
    ]);
    conflictingListSetsFail = expectFailure (resolve [
      { env.X = env.set [ "a" ]; }
      { env.X = env.set [ "b" ]; }
    ]);
    conflictingDefaultsFail = expectFailure (resolve [
      { env.X = env.default "a"; }
      { env.X = env.default "b"; }
    ]);
    conflictingSeparatorsFail = expectFailure (resolve [
      { env.X = env.prepend ":" [ "a" ]; }
      { env.X = env.append " " [ "b" ]; }
    ]);
    invalidNamesFail = expectFailure (resolve [ { env."not-valid" = env.set "a"; } ]);
    conflictingInitialValuesFail = expectFailure (resolve [
      { env.X = env.beforeSetup "a"; }
      { env.X = env.beforeSetup "b"; }
    ]);
    matchingSetsAllowed =
      builtins.isString
        (resolve [
          { env.X = env.set "a"; }
          { env.X = env.set "a"; }
        ]).finalScript;
    replacementBeforeExtension =
      (resolve [
        { env.X = env.prepend ":" [ "prefix" ]; }
        { env.X = env.set "base"; }
      ]).final.X.replacement.value == "base";
    emptyExtensionIsNoop = (resolve [ { env.X = env.prepend ":" [ ]; } ]).final == { };
    emptyExtensionsDoNotConflict =
      (resolve [
        {
          env.X = env.combine [
            (env.prepend ":" [ ])
            (env.append " " [ "value" ])
          ];
        }
      ]).final.X.separator == " ";
    legacyInvalidNameUnchanged =
      (resolve [ { env."legacy.name" = "value"; } ]).initial."legacy.name" == "value";
    nullSetFails = expectFailure (resolve [ { env.X = env.set null; } ]);
    emptySetAllowed =
      lib.hasInfix "_devenv_env_value=''"
        (resolve [ { env.X = env.set ""; } ]).finalScript;
  };
  inherit merged;
}
