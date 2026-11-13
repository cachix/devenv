{
  lib,
  pkgs,
  moduleDirectory ? ../../src/modules/languages,
}:
let
  # Inspect the real generated install scripts without building their tools.
  testPkgs = pkgs // {
    writeShellScript = name: text: builtins.toFile name (builtins.unsafeDiscardStringContext text);
    # Bun is not available on every test platform. Runtime coverage lives in
    # typescript-bun; module composition only needs a versioned derivation.
    bun = pkgs.runCommand "bun-module-test" { version = "1.2.0"; } "touch $out";
  };
  evaluate =
    modules:
    (lib.evalModules {
      specialArgs = {
        pkgs = testPkgs;
        inputs = { };
      };
      modules = [
        (moduleDirectory + "/javascript.nix")
        (moduleDirectory + "/typescript.nix")
        (moduleDirectory + "/../lib.nix")
        {
          options = {
            packages = lib.mkOption {
              type = lib.types.listOf lib.types.package;
              default = [ ];
            };
            enterShell = lib.mkOption {
              type = lib.types.lines;
              default = "";
            };
            assertions = lib.mkOption {
              type = lib.types.listOf lib.types.unspecified;
              default = [ ];
            };
            changelogs = lib.mkOption {
              type = lib.types.listOf lib.types.unspecified;
              default = [ ];
            };
            env = lib.mkOption {
              type = lib.types.submodule {
                freeformType = lib.types.lazyAttrsOf lib.types.anything;
              };
              default = { };
            };
            devenv.root = lib.mkOption {
              type = lib.types.str;
              default = "/project";
            };
            devenv.state = lib.mkOption {
              type = lib.types.str;
              default = "/project/.devenv/state";
            };
          };
          config.env.DEVENV_STATE = "/project/.devenv/state";
        }
      ]
      ++ modules;
    }).config;

  valid = cfg: lib.all (assertion: assertion.assertion) cfg.assertions;
  hasPackage = cfg: package: lib.any (p: toString p == toString package) cfg.packages;
  packageCount =
    cfg: package: builtins.length (builtins.filter (p: toString p == toString package) cfg.packages);
  lines = text: builtins.filter (line: line != "") (lib.splitString "\n" text);
  scripts =
    cfg:
    map (line: builtins.readFile (builtins.elemAt (lib.splitString " " line) 1)) (
      builtins.filter (lib.hasPrefix "source ") (lines cfg.enterShell)
    );
  pathCount =
    cfg: builtins.length (builtins.filter (lib.hasPrefix "export PATH=") (lines cfg.enterShell));

  javascript = evaluate [ { languages.javascript.enable = true; } ];
  typescript = evaluate [ { languages.typescript.enable = true; } ];
  toolingOnly = evaluate [
    {
      languages.typescript = {
        enable = true;
        nodejs.enable = false;
      };
    }
  ];
  bun = evaluate [
    {
      languages.typescript = {
        enable = true;
        bun.enable = true;
        bun.install.enable = true;
      };
    }
  ];
  bunOnly = evaluate [
    {
      languages.typescript = {
        enable = true;
        nodejs.enable = false;
        bun.enable = true;
      };
    }
  ];
  disabled = evaluate [
    {
      languages.javascript.bun = {
        enable = true;
        install.enable = true;
      };
      languages.typescript = {
        bun = {
          enable = true;
          install.enable = true;
          package = throw "disabled Bun package was evaluated";
        };
        deno.enable = true;
      };
    }
  ];
  javascriptDisabled = evaluate [
    {
      languages.javascript = {
        bun.enable = true;
        bun.install.enable = true;
        deno.enable = true;
      };
      languages.typescript.enable = true;
    }
  ];
  customBun = testPkgs.bun.overrideAttrs (_: {
    version = "1.1.0";
    __intentionallyOverridingVersion = true;
  });
  legacyBun = evaluate [
    {
      languages.typescript = {
        enable = true;
        directory = "frontend";
        bun = {
          enable = true;
          package = customBun;
          install.enable = true;
        };
      };
    }
  ];
  both = evaluate [
    {
      languages.javascript = {
        enable = true;
        directory = "./frontend";
        bun.enable = true;
      };
      languages.typescript = {
        enable = true;
        directory = "/project/frontend";
        bun.enable = true;
        bun.install.enable = true;
      };
    }
  ];
  bothInstall = evaluate [
    {
      languages.javascript = {
        enable = true;
        bun.enable = true;
        bun.install.enable = true;
      };
      languages.typescript = {
        enable = true;
        bun.enable = true;
        bun.install.enable = true;
      };
    }
  ];
  separateProjects = evaluate [
    {
      languages.javascript = {
        enable = true;
        directory = "backend";
        bun.enable = true;
        bun.install.enable = true;
      };
      languages.typescript = {
        enable = true;
        directory = "frontend";
        bun.enable = true;
        bun.install.enable = true;
      };
    }
  ];
  conflictingBun = evaluate [
    {
      languages.javascript = {
        enable = true;
        bun.enable = true;
      };
      languages.typescript = {
        enable = true;
        bun.enable = true;
        bun.package = customBun;
      };
    }
  ];
  customNode = pkgs.nodejs-slim.overrideAttrs (_: {
    pname = "custom-nodejs";
  });
  oldPackage = evaluate [
    {
      languages.javascript = {
        enable = true;
        package = customNode;
      };
    }
  ];
  packagePriority = evaluate [
    {
      languages.javascript = {
        enable = true;
        package = lib.mkDefault customNode;
      };
    }
    { languages.javascript.nodejs.package = lib.mkForce pkgs.nodejs-slim; }
  ];
  noLsp = evaluate [
    {
      languages.typescript = {
        enable = true;
        lsp.enable = false;
      };
    }
  ];
  legacyPackagePriority = evaluate [
    {
      languages.javascript = {
        enable = true;
        package = lib.mkForce customNode;
      };
    }
    { languages.javascript.nodejs.package = pkgs.nodejs-slim; }
  ];
  unusedLegacyPackage = evaluate [
    {
      languages.javascript.package = throw "disabled legacy Node.js package was evaluated";
      languages.typescript.enable = true;
    }
  ];
  installOrder = evaluate [
    {
      languages.javascript = {
        enable = true;
      }
      // lib.genAttrs [ "npm" "pnpm" "yarn" "bun" ] (_: {
        enable = true;
        install.enable = true;
      });
    }
  ];
  deno = evaluate [
    {
      languages.typescript = {
        enable = true;
        deno.enable = true;
      };
    }
  ];
  denoOnly = evaluate [
    {
      languages.typescript = {
        enable = true;
        nodejs.enable = false;
        deno.enable = true;
      };
    }
  ];
  legacyDeno = evaluate [ { languages.deno.enable = true; } ];
  allDeno = evaluate [
    {
      languages.deno.enable = true;
      languages.javascript = {
        enable = true;
        deno.enable = true;
      };
      languages.typescript = {
        enable = true;
        deno.enable = true;
      };
    }
  ];
  customDeno = pkgs.deno.overrideAttrs (_: {
    pname = "custom-deno";
  });
  overriddenDeno = evaluate [
    {
      languages.typescript = {
        enable = true;
        deno = {
          enable = true;
          package = customDeno;
        };
      };
    }
  ];
  conflictingDeno = evaluate [
    {
      languages.deno.enable = true;
      languages.typescript = {
        enable = true;
        deno = {
          enable = true;
          package = customDeno;
        };
      };
    }
  ];

  managerChecks =
    manager:
    let
      cfg = evaluate [
        {
          languages.typescript = {
            enable = true;
            ${manager} = {
              enable = true;
              install.enable = true;
            };
          };
        }
      ];
      invalid = evaluate [
        {
          languages.typescript = {
            enable = true;
            ${manager}.install.enable = true;
          };
        }
      ];
      withoutNode = evaluate [
        {
          languages.typescript = {
            enable = true;
            ${manager}.enable = true;
            nodejs.enable = false;
          };
        }
      ];
      shared = evaluate [
        {
          languages.javascript = {
            enable = true;
            ${manager} = {
              enable = true;
              install.enable = true;
            };
          };
          languages.typescript = {
            enable = true;
            ${manager} = {
              enable = true;
              install.enable = true;
            };
          };
        }
      ];
    in
    assert valid cfg;
    assert !valid invalid;
    assert !valid withoutNode;
    assert cfg.languages.typescript.nodejs.enable;
    assert !(cfg.languages.javascript.enable);
    assert builtins.length (scripts cfg) == 1;
    assert lib.hasInfix "${manager}" (builtins.head (scripts cfg));
    assert valid shared;
    assert builtins.length (scripts shared) == 1;
    assert pathCount shared == 1;
    true;
  corepack = evaluate [
    {
      languages.typescript = {
        enable = true;
        corepack.enable = true;
      };
    }
  ];
  corepackWithoutNode = evaluate [
    {
      languages.typescript = {
        enable = true;
        corepack.enable = true;
        nodejs.enable = false;
      };
    }
  ];
  javascriptWithoutNode = evaluate [
    {
      languages.javascript = {
        enable = true;
        nodejs.enable = false;
        bun.enable = true;
      };
    }
  ];
  javascriptBunOnly = evaluate [
    {
      languages.javascript = {
        enable = true;
        nodejs.enable = false;
        lsp.enable = false;
        bun.enable = true;
      };
    }
  ];
in
assert valid javascript;
assert hasPackage javascript pkgs.nodejs-slim;
assert hasPackage javascript pkgs.typescript-language-server;
assert pathCount javascript == 1;
assert valid typescript;
assert hasPackage typescript pkgs.typescript;
assert hasPackage typescript pkgs.typescript-language-server;
assert hasPackage typescript pkgs.nodejs-slim;
assert typescript.languages.typescript.nodejs.enable;
assert !typescript.languages.javascript.enable;
assert pathCount typescript == 1;
assert scripts typescript == [ ];
assert lib.all (name: !typescript.languages.typescript.${name}.enable) [
  "npm"
  "pnpm"
  "yarn"
  "corepack"
  "bun"
  "deno"
];
assert lib.all (name: !javascript.languages.javascript.${name}.enable) [
  "npm"
  "pnpm"
  "yarn"
  "corepack"
  "bun"
  "deno"
];
assert valid toolingOnly;
assert !hasPackage toolingOnly pkgs.nodejs-slim;
assert hasPackage toolingOnly pkgs.typescript;
assert hasPackage toolingOnly pkgs.typescript-language-server;
assert toolingOnly.enterShell == "";
assert valid bun;
assert !bun.languages.javascript.enable;
assert bun.languages.typescript.nodejs.enable;
assert hasPackage bun pkgs.nodejs-slim;
assert hasPackage bun testPkgs.bun;
assert valid bunOnly;
assert !hasPackage bunOnly pkgs.nodejs-slim;
assert hasPackage bunOnly testPkgs.bun;
assert hasPackage bunOnly pkgs.typescript-language-server;
assert lib.hasInfix "/project/node_modules/.bin" bun.enterShell;
assert lib.hasInfix "--save-text-lockfile" (builtins.head (scripts bun));
assert lib.hasSuffix "_devenv-bun-install\nfi\n" (builtins.head (scripts bun));
assert valid disabled;
assert disabled.packages == [ ];
assert disabled.enterShell == "";
assert !(disabled.env ? DENO_DIR);
assert valid javascriptDisabled;
assert !hasPackage javascriptDisabled testPkgs.bun;
assert !hasPackage javascriptDisabled pkgs.deno;
assert valid legacyBun;
assert hasPackage legacyBun customBun;
assert lib.hasSuffix "_devenv-bun-install-legacy\nfi\n" (builtins.head (scripts legacyBun));
assert lib.hasInfix "--cwd frontend" (builtins.head (scripts legacyBun));
assert valid both;
assert builtins.length (scripts both) == 1;
assert pathCount both == 1;
assert packageCount both testPkgs.bun == 1;
assert hasPackage both pkgs.nodejs-slim;
assert valid bothInstall;
assert builtins.length (scripts bothInstall) == 1;
assert valid separateProjects;
assert builtins.length (scripts separateProjects) == 2;
assert pathCount separateProjects == 2;
assert !valid conflictingBun;
assert valid oldPackage;
assert oldPackage.languages.javascript.package == oldPackage.languages.javascript.nodejs.package;
assert hasPackage oldPackage customNode;
assert packagePriority.languages.javascript.package == pkgs.nodejs-slim;
assert legacyPackagePriority.languages.javascript.nodejs.package == customNode;
assert valid unusedLegacyPackage;
assert unusedLegacyPackage.packages == typescript.packages;
assert builtins.length (scripts installOrder) == 4;
assert lib.all
  (
    index:
    lib.hasInfix "_devenv-${builtins.elemAt [ "npm" "pnpm" "yarn" "bun" ] index}-install" (
      builtins.elemAt (scripts installOrder) index
    )
  )
  [
    0
    1
    2
    3
  ];
assert valid noLsp;
assert hasPackage noLsp pkgs.typescript;
assert !hasPackage noLsp pkgs.typescript-language-server;
assert valid deno;
assert !deno.languages.javascript.enable;
assert !deno.languages.deno.enable;
assert deno.languages.typescript.nodejs.enable;
assert hasPackage deno pkgs.deno;
assert hasPackage deno pkgs.nodejs-slim;
assert valid denoOnly;
assert !hasPackage denoOnly pkgs.nodejs-slim;
assert hasPackage denoOnly pkgs.deno;
assert hasPackage denoOnly pkgs.typescript-language-server;
assert deno.env.DENO_INSTALL_ROOT == "/project/.devenv/state/deno";
assert deno.env.DENO_DIR == "/project/.devenv/state/deno/cache";
assert valid legacyDeno;
assert legacyDeno.env == deno.env;
assert valid allDeno;
assert packageCount allDeno pkgs.deno == 1;
assert
  builtins.length (builtins.filter (lib.hasInfix "DENO_INSTALL_ROOT/bin") (lines allDeno.enterShell))
  == 1;
assert valid overriddenDeno;
assert hasPackage overriddenDeno customDeno;
assert !hasPackage overriddenDeno pkgs.deno;
assert !valid conflictingDeno;
assert lib.all managerChecks [
  "npm"
  "pnpm"
  "yarn"
];
assert corepack.languages.typescript.nodejs.enable;
assert valid corepack;
assert !valid corepackWithoutNode;
assert !valid javascriptWithoutNode;
assert valid javascriptBunOnly;
assert !hasPackage javascriptBunOnly pkgs.nodejs-slim;
true
