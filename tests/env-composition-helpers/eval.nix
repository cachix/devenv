{
  nixpkgs ? <nixpkgs>,
  envLibrary ? ../../src/modules/lib/env.nix,
}:
let
  lib = import (nixpkgs + "/lib");
  env = import envLibrary { inherit lib; };
  evaluate =
    modules:
    (lib.evalModules {
      modules = [
        {
          options.env = lib.mkOption {
            type = lib.types.submodule {
              freeformType = lib.types.lazyAttrsOf env.type;
            };
            default = { };
          };
        }
      ]
      ++ modules;
    }).config.env;
  resolve = modules: env.resolve (evaluate modules);
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
    legacyInvalidNameUnchanged =
      (resolve [ { env."legacy.name" = "value"; } ]).initial."legacy.name" == "value";
    nullSetFails = expectFailure (resolve [ { env.X = env.set null; } ]);
    emptySetAllowed =
      lib.hasInfix "_devenv_env_value=''"
        (resolve [ { env.X = env.set ""; } ]).finalScript;
  };
  inherit merged;
}
