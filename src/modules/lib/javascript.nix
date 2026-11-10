{ pkgs
, config
, lib
, ...
}:

let
  languages = [ "javascript" "typescript" ];
  managers = [ "npm" "pnpm" "yarn" "bun" ];
  nodeManagers = [ "npm" "pnpm" "yarn" ];

  absoluteDirectory = directory:
    if lib.hasPrefix "/" directory then
      directory
    else
      "${config.devenv.root}/${directory}";

  projectDirectory = directory: builtins.toPath (absoluteDirectory directory);

  # Keep one installation per package manager and project, even when both
  # languages enable it. Retain JavaScript's existing package and hook order.
  activeLanguages = builtins.filter (language: language.cfg.enable or false) (
    map (name: { inherit name; cfg = config.languages.${name}; }) languages
  );

  managerPackage = cfg: manager:
    if manager == "yarn" then
      cfg.yarn.package.override { nodejs = cfg.nodejs.package; }
    else
      cfg.${manager}.package;

  requests = lib.concatMap
    (language:
      lib.concatMap
        (manager: lib.optional language.cfg.${manager}.enable {
          inherit (language) name cfg;
          inherit manager;
          key = "${manager}:${projectDirectory language.cfg.directory}";
        })
        managers
    )
    activeLanguages;

  groupedRequests = lib.groupBy (request: request.key) requests;
  projects = map
    (key:
      let
        group = groupedRequests.${key};
        first = builtins.head group;
      in
      first // {
        install = lib.any (request: request.cfg.${request.manager}.install.enable) group;
        compatible = lib.all
          (request:
            toString (managerPackage request.cfg request.manager)
            == toString (managerPackage first.cfg first.manager)
          )
          group;
      }
    )
    (lib.unique (map (request: request.key) requests));

  projectsFor = name: builtins.filter (project: project.name == name) projects;

  pathLanguages = builtins.filter
    (language:
      language.name == "javascript"
      || language.cfg.nodejs.enable
      || language.cfg.corepack.enable
      || lib.any (manager: language.cfg.${manager}.enable) managers
    )
    activeLanguages;

  ownsPath = language:
    (lib.findFirst
      (other:
        projectDirectory other.cfg.directory == projectDirectory language.cfg.directory
      )
      { name = null; }
      pathLanguages).name == language.name;

  installScripts = cfg:
    let
      nodeModulesPath = "${
        lib.optionalString (cfg.directory != config.devenv.root) "${cfg.directory}/"
      }node_modules";

      dirPrefix = lib.optionalString (cfg.directory != config.devenv.root) "${cfg.directory}/";
    in
    {

      npm = pkgs.writeShellScript "init-npm.sh" ''
        function _devenv-npm-install()
        {
          # Avoid running "npm install" for every shell.
          # Only run it when the "package-lock.json" file or nodejs version has changed.
          # We do this by storing the nodejs version and a hash of "package-lock.json" in node_modules.
          local ACTUAL_NPM_CHECKSUM="${cfg.npm.package.version}:${config.lib._fileChecksum "${dirPrefix}package-lock.json"}"
          local NPM_CHECKSUM_FILE="${nodeModulesPath}/package-lock.json.checksum"
          if [ -f "$NPM_CHECKSUM_FILE" ]
            then
              read -r EXPECTED_NPM_CHECKSUM < "$NPM_CHECKSUM_FILE"
            else
              EXPECTED_NPM_CHECKSUM=""
          fi

          if [ "$ACTUAL_NPM_CHECKSUM" != "$EXPECTED_NPM_CHECKSUM" ]
          then
            # Use 'clean-install' for reproducible installs from package-lock.json.
            # Fall back to 'install' when no lockfile exists, since 'clean-install' requires one.
            if [ -f "${dirPrefix}package-lock.json" ]
            then
              NPM_CMD="clean-install"
            else
              NPM_CMD="install"
            fi
            if ${lib.getExe' cfg.npm.package "npm"} "$NPM_CMD" ${
              lib.optionalString (cfg.directory != config.devenv.root) "--prefix ${cfg.directory}"
            }
            then
              echo "$ACTUAL_NPM_CHECKSUM" > "$NPM_CHECKSUM_FILE"
            else
              echo "Install failed. Run 'npm $NPM_CMD' manually."
            fi
          fi
        }

        if [ ! -f "${dirPrefix}package.json" ]
        then
          echo "No package.json found${
            lib.optionalString (cfg.directory != config.devenv.root) " in ${cfg.directory}"
          }. Run '${
            lib.optionalString (cfg.directory != config.devenv.root) "cd ${cfg.directory}/ && "
          }npm init' to create one." >&2
        else
          # Keep the lock outside node_modules, which clean-install deletes.
          mkdir -p "${config.devenv.state}"
          {
            if ! ${lib.getExe pkgs.flock} -n 9
            then
              echo "Waiting for another shell to finish installing npm packages..." >&2
              ${lib.getExe pkgs.flock} 9
            fi
            _devenv-npm-install
          } 9>"${config.devenv.state}/npm-install.lock"
        fi
      '';

      pnpm = pkgs.writeShellScript "init-pnpm.sh" ''
        function _devenv-pnpm-install()
        {
          # Avoid running "pnpm install" for every shell.
          # Only run it when the "package-lock.json" file or nodejs version has changed.
          # We do this by storing the nodejs version and a hash of "package-lock.json" in node_modules.
          local ACTUAL_PNPM_CHECKSUM="${cfg.pnpm.package.version}:${config.lib._fileChecksum "${dirPrefix}pnpm-lock.yaml"}"
          local PNPM_CHECKSUM_FILE="${nodeModulesPath}/pnpm-lock.yaml.checksum"
          if [ -f "$PNPM_CHECKSUM_FILE" ]
            then
              read -r EXPECTED_PNPM_CHECKSUM < "$PNPM_CHECKSUM_FILE"
            else
              EXPECTED_PNPM_CHECKSUM=""
          fi

          if [ "$ACTUAL_PNPM_CHECKSUM" != "$EXPECTED_PNPM_CHECKSUM" ]
          then
            # Shell setup has no interactive prompt handling. Only disable the
            # purge confirmation; keep the caller's CI and lockfile policy intact.
            if ${cfg.pnpm.package}/bin/pnpm install --config.confirmModulesPurge=false ${
              lib.optionalString (cfg.directory != config.devenv.root) "--dir ${cfg.directory}"
            }
            then
              echo "$ACTUAL_PNPM_CHECKSUM" > "$PNPM_CHECKSUM_FILE"
            else
              local install_status=$?
              echo "pnpm install failed. Run 'pnpm install' manually." >&2
              return "$install_status"
            fi
          fi
        }

        if [ ! -f "${dirPrefix}package.json" ]
        then
          echo "No package.json found${
            lib.optionalString (cfg.directory != config.devenv.root) " in ${cfg.directory}"
          }. Run '${
            lib.optionalString (cfg.directory != config.devenv.root) "cd ${cfg.directory}/ && "
          }pnpm init' to create one." >&2
        else
          _devenv-pnpm-install
        fi
      '';

      yarn = pkgs.writeShellScript "init-yarn.sh" ''
        function _devenv-yarn-install()
        {
          # Avoid running "yarn install" for every shell.
          # Only run it when the "yarn.lock" file or nodejs version has changed.
          # We do this by storing the nodejs version and a hash of "yarn.lock" in node_modules.
          local ACTUAL_YARN_CHECKSUM="${cfg.yarn.package.version}:${config.lib._fileChecksum "${dirPrefix}yarn.lock"}"
          local YARN_CHECKSUM_FILE="${nodeModulesPath}/yarn.lock.checksum"
          if [ -f "$YARN_CHECKSUM_FILE" ]
            then
              read -r EXPECTED_YARN_CHECKSUM < "$YARN_CHECKSUM_FILE"
            else
              EXPECTED_YARN_CHECKSUM=""
          fi

          if [ "$ACTUAL_YARN_CHECKSUM" != "$EXPECTED_YARN_CHECKSUM" ]
          then
            if ${cfg.yarn.package}/bin/yarn ${
              lib.optionalString (cfg.directory != config.devenv.root) "--cwd ${cfg.directory}"
            } install
            then
              echo "$ACTUAL_YARN_CHECKSUM" > "$YARN_CHECKSUM_FILE"
            else
              echo "Install failed. Run 'yarn install' manually."
            fi
          fi
        }

        if [ ! -f "${dirPrefix}package.json" ]
        then
          echo "No package.json found${
            lib.optionalString (cfg.directory != config.devenv.root) " in ${cfg.directory}"
          }. Run '${
            lib.optionalString (cfg.directory != config.devenv.root) "cd ${cfg.directory}/ && "
          }yarn init' to create one." >&2
        else
          _devenv-yarn-install
        fi
      '';

      bun = pkgs.writeShellScript "init-bun.sh" ''
        # For Bun versions > 1.2, we can use "bun install" directly, since Bun has a readable lockfile
        function _devenv-bun-install()
        {
          # Avoid running "bun install" for every shell.
          # Only run it when the "bun.lock" file or Bun version has changed.
          # Store the Bun version and a checksum of "bun.lock" in node_modules.
          local ACTUAL_BUN_CHECKSUM="${cfg.bun.package.version}:${config.lib._fileChecksum "${dirPrefix}bun.lock"}"
          local BUN_CHECKSUM_FILE="${nodeModulesPath}/bun.lock.checksum"
          if [ -f "$BUN_CHECKSUM_FILE" ]
            then
              read -r EXPECTED_BUN_CHECKSUM < "$BUN_CHECKSUM_FILE"
            else
              EXPECTED_BUN_CHECKSUM=""
          fi

          if [ "$ACTUAL_BUN_CHECKSUM" != "$EXPECTED_BUN_CHECKSUM" ]
          then
            if ${cfg.bun.package}/bin/bun install --save-text-lockfile ${
              lib.optionalString (cfg.directory != config.devenv.root) "--cwd ${cfg.directory}"
            }
            then
              echo "$ACTUAL_BUN_CHECKSUM" > "$BUN_CHECKSUM_FILE"

              if [ -f "${cfg.directory}/yarn.lock" ]
              then
                echo "yarn.lock is no longer needed when using bun."
                echo "Starting with Bun 1.2, bun uses its own lockfile (bun.lock)."
                echo "You can remove yarn.lock."
              fi
            else
              echo "Install failed. Run 'bun install' manually."
            fi
          fi
        }

        # For Bun versions <= 1.2, we need to use "bun install --yarn", since Bun didn't have readable lockfiles then
        function _devenv-bun-install-legacy()
        {
          # Avoid running "bun install --yarn" for every shell.
          # Only run it when the "yarn.lock" file or Bun version has changed.
          # Store the Bun version and a checksum of "yarn.lock" in node_modules.
          local ACTUAL_BUN_CHECKSUM="${cfg.bun.package.version}:${config.lib._fileChecksum "${dirPrefix}yarn.lock"}"
          local BUN_CHECKSUM_FILE="${nodeModulesPath}/yarn.lock.checksum"
          if [ -f "$BUN_CHECKSUM_FILE" ]
            then
              read -r EXPECTED_BUN_CHECKSUM < "$BUN_CHECKSUM_FILE"
            else
              EXPECTED_BUN_CHECKSUM=""
          fi

          if [ "$ACTUAL_BUN_CHECKSUM" != "$EXPECTED_BUN_CHECKSUM" ]
          then
            if ${cfg.bun.package}/bin/bun install --yarn ${
              lib.optionalString (cfg.directory != config.devenv.root) "--cwd ${cfg.directory}"
            }
            then
              echo "$ACTUAL_BUN_CHECKSUM" > "$BUN_CHECKSUM_FILE"
            else
              echo "Install failed. Run 'bun install --yarn' manually."
            fi
          fi
        }

        if [ ! -f "${dirPrefix}package.json" ]
        then
          echo "No package.json found${
            lib.optionalString (cfg.directory != config.devenv.root) " in ${cfg.directory}"
          }. Run '${
            lib.optionalString (cfg.directory != config.devenv.root) "cd ${cfg.directory}/ && "
          }bun init' to create one." >&2
        else
          ${
            if (lib.strings.versionAtLeast cfg.bun.package.version "1.2") then
              "_devenv-bun-install"
            else
              "_devenv-bun-install-legacy"
          }
        fi
      '';
    };

  languageOptions = name:
    let
      cfg = config.languages.${name};
      pnpmArgs = lib.functionArgs pkgs.pnpm.override;
      pnpmPackage =
        if builtins.hasAttr "nodejs-slim" pnpmArgs then
          pkgs.pnpm.override { nodejs-slim = cfg.nodejs.package; }
        else
          pkgs.pnpm.override { nodejs = cfg.nodejs.package; };
    in
    {

      directory = lib.mkOption {
        type = lib.types.str;
        default = config.devenv.root;
        defaultText = lib.literalExpression "config.devenv.root";
        description = ''
          The ${name} project's root directory. Defaults to the root of the devenv project.
          Can be an absolute path or one relative to the root of the devenv project.
        '';
        example = "./directory";
      };

      nodejs = {
        enable = lib.mkEnableOption "Node.js runtime" // {
          default = name == "javascript"
            || cfg.corepack.enable
            || lib.any (manager: cfg.${manager}.enable) nodeManagers;
          defaultText = if name == "javascript" then lib.literalExpression "true" else
          lib.literalExpression "npm.enable || pnpm.enable || yarn.enable || corepack.enable";
        };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.nodejs-slim;
          defaultText = lib.literalExpression "pkgs.nodejs-slim";
          description = "The Node.js package to use.";
        };
      };

      corepack = {
        enable = lib.mkEnableOption "wrappers for npm, pnpm and Yarn via Node.js Corepack";
      };

      npm = {
        enable = lib.mkEnableOption "install npm";
        package = lib.mkOption {
          type = lib.types.package;
          default =
            # nixpkgs 26.05+: nodejs-slim has an npm output
            if builtins.elem "npm" (cfg.nodejs.package.outputs or [ ])
            then lib.getOutput "npm" cfg.nodejs.package
            # older nixpkgs: enableNpm override
            else if cfg.nodejs.package ? override && lib.functionArgs cfg.nodejs.package.override ? enableNpm
            then cfg.nodejs.package.override { enableNpm = true; }
            else cfg.nodejs.package;
          defaultText = lib.literalExpression "languages.${name}.nodejs.package";
          description = "The npm package to use. Defaults to the npm output of the Node.js package.";
        };
        install.enable = lib.mkEnableOption "npm install during devenv initialisation";
      };

      pnpm = {
        enable = lib.mkEnableOption "install pnpm";
        package = lib.mkOption {
          type = lib.types.package;
          default = pnpmPackage;
          defaultText = lib.literalExpression "pkgs.pnpm";
          description = "The pnpm package to use.";
        };
        install.enable = lib.mkEnableOption "pnpm install during devenv initialisation";
      };

      yarn = {
        enable = lib.mkEnableOption "install yarn";
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.yarn.override {
            nodejs = cfg.nodejs.package;
          };
          defaultText = lib.literalExpression "pkgs.yarn";
          description = "The yarn package to use.";
        };
        install.enable = lib.mkEnableOption "yarn install during devenv initialisation";
      };

      bun = {
        enable = lib.mkEnableOption "install bun";
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.bun;
          defaultText = lib.literalExpression "pkgs.bun";
          description = "The bun package to use.";
        };
        install.enable = lib.mkEnableOption "bun install during devenv initialisation";
      };

      deno = {
        enable = lib.mkEnableOption "tools for Deno development";
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.deno;
          defaultText = lib.literalExpression "pkgs.deno";
          description = "The Deno package to use.";
        };
      };

      lsp = {
        enable = lib.mkEnableOption
          (
            if name == "javascript" then "TypeScript Language Server for JavaScript" else "TypeScript Language Server"
          ) // { default = true; };
        package = lib.mkOption {
          type = lib.types.package;
          default = pkgs.typescript-language-server;
          defaultText = lib.literalExpression "pkgs.typescript-language-server";
          description = "The TypeScript/JavaScript language server package to use.";
        };
      };
    };
in
{
  imports = [ ../languages/deno.nix ];

  options.languages = lib.genAttrs languages languageOptions;

  config = {
    packages = lib.concatMap
      (language:
        let
          inherit (language) cfg;
          # On nixpkgs 26.05+, nodejs-slim has a corepack output.
          corepack = cfg.nodejs.package.corepack or cfg.nodejs.package;
        in
        lib.optional cfg.nodejs.enable cfg.nodejs.package
        ++ map (project: managerPackage project.cfg project.manager) (projectsFor language.name)
        ++ lib.optionals cfg.corepack.enable [
          corepack
          (pkgs.runCommand "corepack-enable" { } ''
            mkdir -p $out/bin
            ${corepack}/bin/corepack enable --install-directory $out/bin
          '')
        ]
        ++ lib.optional cfg.lsp.enable cfg.lsp.package
      )
      activeLanguages;

    enterShell = lib.concatStringsSep "\n" (lib.concatMap
      (language:
        map
          (project: ''
            source ${(installScripts project.cfg).${project.manager}}${lib.optionalString (project.manager == "pnpm") " || exit $?"}
          '')
          (builtins.filter (project: project.install) (projectsFor language.name))
        ++ lib.optional (ownsPath language) ''
          export PATH="${absoluteDirectory language.cfg.directory}/node_modules/.bin:$PATH"
        ''
      )
      activeLanguages);

    assertions = lib.concatMap
      (language:
        let inherit (language) name cfg; in
        map
          (manager: {
            assertion = cfg.${manager}.enable -> cfg.nodejs.enable;
            message = "languages.${name}.${manager}.enable requires languages.${name}.nodejs.enable = true;";
          })
          (nodeManagers ++ [ "corepack" ])
        ++ map
          (manager: {
            assertion = cfg.${manager}.install.enable -> cfg.${manager}.enable;
            message = "languages.${name}.${manager}.install.enable requires languages.${name}.${manager}.enable = true;";
          })
          managers
      )
      activeLanguages
    ++ map
      (project: {
        assertion = project.compatible;
        message = "languages.javascript.${project.manager}.package and languages.typescript.${project.manager}.package must match when their project directories are the same;";
      })
      projects;
  };
}
