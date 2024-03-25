# devenv.nix options

## packages



A list of packages to expose inside the developer environment. Search available packages using ` devenv search NAME `.



*Type:*
list of package



*Default:*
` [ ] `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/top-level.nix](https://github.com/cachix/devenv/blob/main/src/modules/top-level.nix)



## aws-vault.enable



Whether to enable aws-vault integration.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/aws-vault.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/aws-vault.nix)



## aws-vault.package



The aws-vault package to use.



*Type:*
package



*Default:*
` pkgs.aws-vault `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/aws-vault.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/aws-vault.nix)



## aws-vault.awscliWrapper

Attribute set of packages including awscli2



*Type:*
submodule



*Default:*
` pkgs `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/aws-vault.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/aws-vault.nix)



## aws-vault.awscliWrapper.enable



Whether to enable Wraps awscli2 binary as ` aws-vault exec <profile> -- aws <args> `.
.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/aws-vault.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/aws-vault.nix)



## aws-vault.awscliWrapper.package



The awscli2 package to use.



*Type:*
package



*Default:*
` pkgs.awscli2 `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/aws-vault.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/aws-vault.nix)



## aws-vault.opentofuWrapper



Attribute set of packages including opentofu



*Type:*
submodule



*Default:*
` pkgs `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/aws-vault.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/aws-vault.nix)



## aws-vault.opentofuWrapper.enable



Whether to enable Wraps opentofu binary as ` aws-vault exec <profile> -- opentofu <args> `.
.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/aws-vault.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/aws-vault.nix)



## aws-vault.opentofuWrapper.package



The opentofu package to use.



*Type:*
package



*Default:*
` pkgs.opentofu `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/aws-vault.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/aws-vault.nix)



## aws-vault.profile



The profile name passed to ` aws-vault exec `.



*Type:*
string

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/aws-vault.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/aws-vault.nix)



## aws-vault.terraformWrapper



Attribute set of packages including terraform



*Type:*
submodule



*Default:*
` pkgs `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/aws-vault.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/aws-vault.nix)



## aws-vault.terraformWrapper.enable



Whether to enable Wraps terraform binary as ` aws-vault exec <profile> -- terraform <args> `.
.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/aws-vault.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/aws-vault.nix)



## aws-vault.terraformWrapper.package



The terraform package to use.



*Type:*
package



*Default:*
` pkgs.terraform `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/aws-vault.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/aws-vault.nix)



## cachix.enable



Whether to enable Cachix integration.



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/cachix.nix](https://github.com/cachix/devenv/blob/main/src/modules/cachix.nix)



## cachix.pull



What caches to pull from.



*Type:*
list of string

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/cachix.nix](https://github.com/cachix/devenv/blob/main/src/modules/cachix.nix)



## cachix.push



What cache to push to. Automatically also adds it to the list of caches to pull from.



*Type:*
null or string



*Default:*
` null `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/cachix.nix](https://github.com/cachix/devenv/blob/main/src/modules/cachix.nix)



## certificates



List of domains to generate certificates for.



*Type:*
list of string



*Default:*
` [ ] `



*Example:*

```
[
  "example.com"
  "*.example.com"
]
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/mkcert.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/mkcert.nix)



## container.isBuilding



Set to true when the environment is building a container.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/containers.nix](https://github.com/cachix/devenv/blob/main/src/modules/containers.nix)



## containers



Container specifications that can be built, copied and ran using ` devenv container `.



*Type:*
attribute set of (submodule)



*Default:*
` { } `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/containers.nix](https://github.com/cachix/devenv/blob/main/src/modules/containers.nix)



## containers.\<name>.copyToRoot



Add a path to the container. Defaults to the whole git repo.



*Type:*
null or path or list of path



*Default:*
` "self" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/containers.nix](https://github.com/cachix/devenv/blob/main/src/modules/containers.nix)



## containers.\<name>.defaultCopyArgs



Default arguments to pass to ` skopeo copy `.
You can override them by passing arguments to the script.



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/containers.nix](https://github.com/cachix/devenv/blob/main/src/modules/containers.nix)



## containers.\<name>.entrypoint



Entrypoint of the container.



*Type:*
list of anything



*Default:*
` [ entrypoint ] `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/containers.nix](https://github.com/cachix/devenv/blob/main/src/modules/containers.nix)



## containers.\<name>.isBuilding



Set to true when the environment is building this container.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/containers.nix](https://github.com/cachix/devenv/blob/main/src/modules/containers.nix)



## containers.\<name>.maxLayers



Maximum number of container layers created.



*Type:*
null or signed integer



*Default:*
` 1 `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/containers.nix](https://github.com/cachix/devenv/blob/main/src/modules/containers.nix)



## containers.\<name>.name



Name of the container.



*Type:*
null or string



*Default:*
` "top-level name or containers.mycontainer.name" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/containers.nix](https://github.com/cachix/devenv/blob/main/src/modules/containers.nix)



## containers.\<name>.registry



Registry to push the container to.



*Type:*
null or string



*Default:*
` "docker-daemon:" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/containers.nix](https://github.com/cachix/devenv/blob/main/src/modules/containers.nix)



## containers.\<name>.startupCommand



Command to run in the container.



*Type:*
null or string or package



*Default:*
` null `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/containers.nix](https://github.com/cachix/devenv/blob/main/src/modules/containers.nix)



## containers.\<name>.version



Version/tag of the container.



*Type:*
null or string



*Default:*
` "latest" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/containers.nix](https://github.com/cachix/devenv/blob/main/src/modules/containers.nix)



## delta.enable



Integrate delta into git: https://dandavison.github.io/delta/.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/delta.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/delta.nix)



## devcontainer.enable



Whether to enable generation .devcontainer.json for devenv integration.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/devcontainer.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/devcontainer.nix)



## devcontainer.settings



Devcontainer settings.



*Type:*
JSON value



*Default:*
` { } `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/devcontainer.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/devcontainer.nix)



## devcontainer.settings.customizations.vscode.extensions



List of preinstalled VSCode extensions.



*Type:*
list of string



*Default:*

```
[
  "mkhl.direnv"
]
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/devcontainer.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/devcontainer.nix)



## devcontainer.settings.image



The name of an image in a container registry.



*Type:*
string



*Default:*
` "ghcr.io/cachix/devenv:latest" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/devcontainer.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/devcontainer.nix)



## devcontainer.settings.overrideCommand



Override the default command.



*Type:*
anything



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/devcontainer.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/devcontainer.nix)



## devcontainer.settings.updateContentCommand



Command to run after container creation.



*Type:*
anything



*Default:*
` "devenv test" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/devcontainer.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/devcontainer.nix)



## devenv.debug



Whether to enable debug mode of devenv enterShell script.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/debug.nix](https://github.com/cachix/devenv/blob/main/src/modules/debug.nix)



## devenv.flakesIntegration



Tells if devenv is being imported by a flake.nix file



*Type:*
boolean



*Default:*
` true ` when devenv is invoked via the flake integration; ` false ` otherwise.

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/update-check.nix](https://github.com/cachix/devenv/blob/main/src/modules/update-check.nix)



## devenv.latestVersion



The latest version of devenv.



*Type:*
string



*Default:*
` "1.0.2" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/update-check.nix](https://github.com/cachix/devenv/blob/main/src/modules/update-check.nix)



## devenv.warnOnNewVersion



Whether to warn when a new version of devenv is available.



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/update-check.nix](https://github.com/cachix/devenv/blob/main/src/modules/update-check.nix)



## difftastic.enable



Integrate difftastic into git: https://difftastic.wilfred.me.uk/.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/difftastic.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/difftastic.nix)



## dotenv.enable



Whether to enable .env integration, doesn’t support comments or multiline values…



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/dotenv.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/dotenv.nix)



## dotenv.disableHint



Disable the hint that are printed when the dotenv module is not enabled, but .env is present.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/dotenv.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/dotenv.nix)



## dotenv.filename



The name of the dotenv file to load, or a list of dotenv files to load in order of precedence.



*Type:*
string or list of string



*Default:*
` ".env" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/dotenv.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/dotenv.nix)



## enterShell



Bash code to execute when entering the shell.



*Type:*
strings concatenated with “\\n”



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/top-level.nix](https://github.com/cachix/devenv/blob/main/src/modules/top-level.nix)



## enterTest



Bash code to execute to run the test.



*Type:*
strings concatenated with “\\n”

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/tests.nix](https://github.com/cachix/devenv/blob/main/src/modules/tests.nix)



## env



Environment variables to be exposed inside the developer environment.



*Type:*
lazy attribute set of anything



*Default:*
` { } `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/top-level.nix](https://github.com/cachix/devenv/blob/main/src/modules/top-level.nix)



## hosts



List of hosts entries.



*Type:*
attribute set of (string or list of string)



*Default:*
` { } `



*Example:*

```
{
  "another-example.com" = [
    "::1"
    "127.0.0.1"
  ];
  "example.com" = "127.0.0.1";
}
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/hostctl.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/hostctl.nix)



## hostsProfileName



Profile name to use.



*Type:*
string



*Default:*
` "devenv-16a3de0b53062f3b6e6678a84f28e04344732e0002fcad6af20ffbeaa0491014" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/hostctl.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/hostctl.nix)



## infoSections



Information about the environment



*Type:*
attribute set of list of string



*Default:*
` { } `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/info.nix](https://github.com/cachix/devenv/blob/main/src/modules/info.nix)



## languages.ansible.enable



Whether to enable tools for Ansible development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/ansible.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/ansible.nix)



## languages.ansible.package



The Ansible package to use.



*Type:*
package



*Default:*
` pkgs.ansible `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/ansible.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/ansible.nix)



## languages.c.enable



Whether to enable tools for C development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/c.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/c.nix)



## languages.clojure.enable



Whether to enable tools for Clojure development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/clojure.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/clojure.nix)



## languages.cplusplus.enable



Whether to enable tools for C++ development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/cplusplus.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/cplusplus.nix)



## languages.crystal.enable



Whether to enable Enable tools for Crystal development…



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/crystal.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/crystal.nix)



## languages.cue.enable



Whether to enable tools for Cue development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/cue.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/cue.nix)



## languages.cue.package



The CUE package to use.



*Type:*
package



*Default:*
` pkgs.cue `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/cue.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/cue.nix)



## languages.dart.enable



Whether to enable tools for Dart development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/dart.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/dart.nix)



## languages.dart.package



The Dart package to use.



*Type:*
package



*Default:*
` pkgs.dart `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/dart.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/dart.nix)



## languages.deno.enable



Whether to enable tools for Deno development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/deno.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/deno.nix)



## languages.dotnet.enable



Whether to enable tools for .NET development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/dotnet.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/dotnet.nix)



## languages.dotnet.package



The .NET SDK package to use.



*Type:*
package



*Default:*
` pkgs.dotnet-sdk `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/dotnet.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/dotnet.nix)



## languages.elixir.enable



Whether to enable tools for Elixir development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix)



## languages.elixir.package



Which package of Elixir to use.



*Type:*
package



*Default:*
` pkgs.elixir `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix)



## languages.elm.enable



Whether to enable tools for Elm development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/elm.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/elm.nix)



## languages.erlang.enable



Whether to enable tools for Erlang development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/erlang.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/erlang.nix)



## languages.erlang.package



Which package of Erlang to use.



*Type:*
package



*Default:*
` pkgs.erlang `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/erlang.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/erlang.nix)



## languages.fortran.enable



Whether to enable tools for Fortran Development…



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/fortran.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/fortran.nix)



## languages.fortran.package



The Fortran package to use.



*Type:*
package



*Default:*
` pkgs.gfortran `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/fortran.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/fortran.nix)



## languages.gawk.enable



Whether to enable tools for GNU Awk development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/gawk.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/gawk.nix)



## languages.gleam.enable



Whether to enable tools for Gleam development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/gleam.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/gleam.nix)



## languages.gleam.package



The Gleam package to use.



*Type:*
package



*Default:*
` pkgs.gleam `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/gleam.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/gleam.nix)



## languages.go.enable



Whether to enable tools for Go development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/go.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/go.nix)



## languages.go.package



The Go package to use.



*Type:*
package



*Default:*
` pkgs.go `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/go.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/go.nix)



## languages.haskell.enable



Whether to enable tools for Haskell development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/haskell.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/haskell.nix)



## languages.haskell.package



Haskell compiler to use.



*Type:*
package



*Default:*
` "pkgs.ghc" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/haskell.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/haskell.nix)



## languages.haskell.languageServer



Haskell language server to use.



*Type:*
null or package



*Default:*
` "pkgs.haskell-language-server" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/haskell.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/haskell.nix)



## languages.haskell.stack



Haskell stack to use.



*Type:*
null or package



*Default:*
` "pkgs.stack" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/haskell.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/haskell.nix)



## languages.idris.enable



Whether to enable tools for Idris development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/idris.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/idris.nix)



## languages.idris.package



The Idris package to use.



*Type:*
package



*Default:*
` "pkgs.idris2" `



*Example:*
` "pkgs.idris" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/idris.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/idris.nix)



## languages.java.enable



Whether to enable tools for Java development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/java.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/java.nix)



## languages.java.gradle.enable



Whether to enable gradle.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/java.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/java.nix)



## languages.java.gradle.package



The Gradle package to use.
The Gradle package by default inherits the JDK from ` languages.java.jdk.package `.



*Type:*
package



*Default:*
` pkgs.gradle.override { jdk = cfg.jdk.package; } `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/java.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/java.nix)



## languages.java.jdk.package



The JDK package to use.
This will also become available as ` JAVA_HOME `.



*Type:*
package



*Default:*
` pkgs.jdk `



*Example:*
` pkgs.jdk8 `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/java.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/java.nix)



## languages.java.maven.enable



Whether to enable maven.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/java.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/java.nix)



## languages.java.maven.package



The Maven package to use.
The Maven package by default inherits the JDK from ` languages.java.jdk.package `.



*Type:*
package



*Default:*
` "pkgs.maven.override { jdk = cfg.jdk.package; }" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/java.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/java.nix)



## languages.javascript.enable



Whether to enable tools for JavaScript development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix)



## languages.javascript.package



The Node.js package to use.



*Type:*
package



*Default:*
` pkgs.nodejs-slim `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix)



## languages.javascript.bun.enable



Whether to enable install bun.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix)



## languages.javascript.bun.package



The bun package to use.



*Type:*
package



*Default:*
` pkgs.bun `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix)



## languages.javascript.bun.install.enable



Whether to enable bun install during devenv initialisation.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix)



## languages.javascript.corepack.enable



Whether to enable wrappers for npm, pnpm and Yarn via Node.js Corepack.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix)



## languages.javascript.directory



The JavaScript project’s root directory. Defaults to the root of the devenv project.
Can be an absolute path or one relative to the root of the devenv project.



*Type:*
string



*Default:*
` config.devenv.root `



*Example:*
` "./directory" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix)



## languages.javascript.npm.enable



Whether to enable install npm.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix)



## languages.javascript.npm.package



The Node.js package to use.



*Type:*
package



*Default:*
` pkgs.nodejs `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix)



## languages.javascript.npm.install.enable



Whether to enable npm install during devenv initialisation.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix)



## languages.javascript.pnpm.enable



Whether to enable install pnpm.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix)



## languages.javascript.pnpm.package



The pnpm package to use.



*Type:*
package



*Default:*
` pkgs.nodePackages.pnpm `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix)



## languages.javascript.pnpm.install.enable



Whether to enable pnpm install during devenv initialisation.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix)



## languages.javascript.yarn.enable



Whether to enable install yarn.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix)



## languages.javascript.yarn.package

The yarn package to use.



*Type:*
package



*Default:*
` pkgs.yarn `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix)



## languages.javascript.yarn.install.enable



Whether to enable yarn install during devenv initialisation.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix)



## languages.jsonnet.enable



Whether to enable tools for jsonnet development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/jsonnet.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/jsonnet.nix)



## languages.julia.enable



Whether to enable tools for Julia development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/julia.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/julia.nix)



## languages.julia.package



The Julia package to use.



*Type:*
package



*Default:*
` pkgs.julia-bin `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/julia.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/julia.nix)



## languages.kotlin.enable



Whether to enable tools for Kotlin development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/kotlin.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/kotlin.nix)



## languages.lua.enable



Whether to enable tools for Lua development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/lua.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/lua.nix)



## languages.lua.package



The Lua package to use.



*Type:*
package



*Default:*
` pkgs.lua `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/lua.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/lua.nix)



## languages.nim.enable



Whether to enable tools for Nim development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/nim.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/nim.nix)



## languages.nim.package



The Nim package to use.



*Type:*
package



*Default:*
` pkgs.nim `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/nim.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/nim.nix)



## languages.nix.enable



Whether to enable tools for Nix development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/nix.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/nix.nix)



## languages.nix.lsp.package



The LSP package to use



*Type:*
package



*Default:*
` pkgs.nil `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/nix.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/nix.nix)



## languages.ocaml.enable



Whether to enable tools for OCaml development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/ocaml.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/ocaml.nix)



## languages.ocaml.packages



The package set of OCaml to use



*Type:*
attribute set



*Default:*
` pkgs.ocaml-ng.ocamlPackages_4_12 `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/ocaml.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/ocaml.nix)



## languages.opentofu.enable



Whether to enable tools for OpenTofu development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/opentofu.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/opentofu.nix)



## languages.opentofu.package



The OpenTofu package to use.



*Type:*
package



*Default:*
` pkgs.opentofu `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/opentofu.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/opentofu.nix)



## languages.pascal.enable



Whether to enable tools for Pascal development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/pascal.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/pascal.nix)



## languages.pascal.lazarus.enable



Whether to enable lazarus graphical IDE for the FreePascal language.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/pascal.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/pascal.nix)



## languages.perl.enable



Whether to enable tools for Perl development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/perl.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/perl.nix)



## languages.perl.packages



Perl packages to include



*Type:*
list of string



*Default:*
` [ ] `



*Example:*

```
[
  "Mojolicious"
]
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/perl.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/perl.nix)



## languages.php.enable



Whether to enable tools for PHP development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages.php.package



Allows you to [override the default used package](https://nixos.org/manual/nixpkgs/stable/\#ssec-php-user-guide)
to adjust the settings or add more extensions. You can find the
extensions using ` devenv search 'php extensions' `



*Type:*
package



*Default:*
` pkgs.php `



*Example:*

```
pkgs.php.buildEnv {
  extensions = { all, enabled }: with all; enabled ++ [ xdebug ];
  extraConfig = ''
    memory_limit=1G
  '';
};

```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages.php.packages



Attribute set of packages including composer



*Type:*
submodule



*Default:*
` pkgs `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages.php.packages.composer



composer package



*Type:*
null or package



*Default:*
` pkgs.phpPackages.composer `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages.php.disableExtensions



PHP extensions to disable.



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages.php.extensions



PHP extensions to enable.



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages.php.fpm.extraConfig



Extra configuration that should be put in the global section of
the PHP-FPM configuration file. Do not specify the options
` error_log ` or ` daemonize ` here, since they are generated by
NixOS.



*Type:*
null or strings concatenated with “\\n”



*Default:*
` null `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages.php.fpm.phpOptions



Options appended to the PHP configuration file ` php.ini `.



*Type:*
strings concatenated with “\\n”



*Default:*
` "" `



*Example:*

```
''
  date.timezone = "CET"
''
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages.php.fpm.pools



PHP-FPM pools. If no pools are defined, the PHP-FPM
service is disabled.



*Type:*
attribute set of (submodule)



*Default:*
` { } `



*Example:*

```
{
  mypool = {
    user = "php";
    group = "php";
    phpPackage = pkgs.php;
    settings = {
      "pm" = "dynamic";
      "pm.max_children" = 75;
      "pm.start_servers" = 10;
      "pm.min_spare_servers" = 5;
      "pm.max_spare_servers" = 20;
      "pm.max_requests" = 500;
    };
  }
}
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages.php.fpm.pools.\<name>.extraConfig



Extra lines that go into the pool configuration.
See the documentation on ` php-fpm.conf ` for
details on configuration directives.



*Type:*
null or strings concatenated with “\\n”



*Default:*
` null `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages.php.fpm.pools.\<name>.listen



The address on which to accept FastCGI requests.



*Type:*
string



*Default:*
` "" `



*Example:*
` "/path/to/unix/socket" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages.php.fpm.pools.\<name>.phpEnv



Environment variables used for this PHP-FPM pool.



*Type:*
attribute set of string



*Default:*
` { } `



*Example:*

```
{
  HOSTNAME = "$HOSTNAME";
  TMP = "/tmp";
  TMPDIR = "/tmp";
  TEMP = "/tmp";
}

```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages.php.fpm.pools.\<name>.phpOptions



Options appended to the PHP configuration file ` php.ini ` used for this PHP-FPM pool.



*Type:*
strings concatenated with “\\n”

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages.php.fpm.pools.\<name>.phpPackage



The PHP package to use for running this PHP-FPM pool.



*Type:*
package



*Default:*
` phpfpm.phpPackage `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages.php.fpm.pools.\<name>.settings



PHP-FPM pool directives. Refer to the “List of pool directives” section of
[https://www.php.net/manual/en/install.fpm.configuration.php"](https://www.php.net/manual/en/install.fpm.configuration.php%22)
the manual for details. Note that settings names must be
enclosed in quotes (e.g. ` "pm.max_children" ` instead of
` pm.max_children `).



*Type:*
attribute set of (string or signed integer or boolean)



*Default:*
` { } `



*Example:*

```
{
  "pm" = "dynamic";
  "pm.max_children" = 75;
  "pm.start_servers" = 10;
  "pm.min_spare_servers" = 5;
  "pm.max_spare_servers" = 20;
  "pm.max_requests" = 500;
}

```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages.php.fpm.pools.\<name>.socket



Path to the Unix socket file on which to accept FastCGI requests.

This option is read-only and managed by NixOS.



*Type:*
string *(read only)*



*Example:*
` "/home/runner/work/devenv/devenv/.devenv/state/php-fpm/<name>.sock" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages.php.fpm.settings



PHP-FPM global directives.

Refer to the “List of global php-fpm.conf directives” section of
[https://www.php.net/manual/en/install.fpm.configuration.php](https://www.php.net/manual/en/install.fpm.configuration.php)
for details.

Note that settings names must be enclosed in
quotes (e.g. ` "pm.max_children" ` instead of ` pm.max_children `).

You need not specify the options ` error_log ` or ` daemonize ` here, since
they are already set.



*Type:*
attribute set of (string or signed integer or boolean)



*Default:*

```
{
  error_log = "/home/runner/work/devenv/devenv/.devenv/state/php-fpm/php-fpm.log";
}
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages.php.ini



PHP.ini directives. Refer to the “List of php.ini directives” of PHP’s



*Type:*
null or strings concatenated with “\\n”



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages.php.version



The PHP version to use.



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages.purescript.enable



Whether to enable tools for PureScript development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/purescript.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/purescript.nix)



## languages.purescript.package



The PureScript package to use.



*Type:*
package



*Default:*
` pkgs.purescript `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/purescript.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/purescript.nix)



## languages.python.enable



Whether to enable tools for Python development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix)



## languages.python.package



The Python package to use.



*Type:*
package



*Default:*
` pkgs.python3 `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix)



## languages.python.directory



The Python project’s root directory. Defaults to the root of the devenv project.
Can be an absolute path or one relative to the root of the devenv project.



*Type:*
string



*Default:*
` config.devenv.root `



*Example:*
` "./directory" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix)



## languages.python.libraries



Additional libraries to make available to the Python interpreter.

This is useful when you want to use Python wheels that depend on native libraries.



*Type:*
list of path



*Default:*

```
[
  "/home/runner/work/devenv/devenv/.devenv/profile"
]
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix)



## languages.python.manylinux.enable



Whether to install manylinux2014 libraries.

Enabled by default on linux;

This is useful when you want to use Python wheels that depend on manylinux2014 libraries.



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix)



## languages.python.poetry.enable



Whether to enable poetry.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix)



## languages.python.poetry.package



The Poetry package to use.



*Type:*
package



*Default:*
` pkgs.poetry `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix)



## languages.python.poetry.activate.enable



Whether to activate the poetry virtual environment automatically.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix)



## languages.python.poetry.install.enable



Whether to enable poetry install during devenv initialisation.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix)



## languages.python.poetry.install.allExtras



Whether to install all extras. See ` --all-extras `.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix)



## languages.python.poetry.install.compile



Whether ` poetry install ` should compile Python source files to bytecode.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix)



## languages.python.poetry.install.extras



Which extras to install. See ` --extras `.



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix)



## languages.python.poetry.install.groups



Which dependency groups to install. See ` --with `.



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix)



## languages.python.poetry.install.ignoredGroups



Which dependency groups to ignore. See ` --without `.



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix)



## languages.python.poetry.install.installRootPackage



Whether the root package (your project) should be installed. See ` --no-root `



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix)



## languages.python.poetry.install.onlyGroups



Which dependency groups to exclusively install. See ` --only `.



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix)



## languages.python.poetry.install.onlyInstallRootPackage



Whether to only install the root package (your project) should be installed, but no dependencies. See ` --only-root `



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix)



## languages.python.poetry.install.quiet



Whether ` poetry install ` should avoid outputting messages during devenv initialisation.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix)



## languages.python.poetry.install.verbosity



What level of verbosity the output of ` poetry install ` should have.



*Type:*
one of “no”, “little”, “more”, “debug”



*Default:*
` "no" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix)



## languages.python.venv.enable



Whether to enable Python virtual environment.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix)



## languages.python.venv.quiet



Whether ` pip install ` should avoid outputting messages during devenv initialisation.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix)



## languages.python.venv.requirements



Contents of pip requirements.txt file.
This is passed to ` pip install -r ` during ` devenv shell ` initialisation.



*Type:*
null or strings concatenated with “\\n” or path



*Default:*
` null `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix)



## languages.python.version



The Python version to use.
This automatically sets the ` languages.python.package ` using [nixpkgs-python](https://github.com/cachix/nixpkgs-python).



*Type:*
null or string



*Default:*
` null `



*Example:*
` "3.11 or 3.11.2" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix)



## languages.r.enable



Whether to enable tools for R development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/r.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/r.nix)



## languages.r.package



The R package to use.



*Type:*
package



*Default:*
` pkgs.R `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/r.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/r.nix)



## languages.racket.enable



Whether to enable tools for Racket development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/racket.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/racket.nix)



## languages.racket.package



The Racket package to use.



*Type:*
package



*Default:*
` pkgs.racket-minimal `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/racket.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/racket.nix)



## languages.raku.enable



Whether to enable tools for Raku development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/raku.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/raku.nix)



## languages.robotframework.enable



Whether to enable tools for Robot Framework development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/robotframework.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/robotframework.nix)



## languages.robotframework.python



The Python package to use.



*Type:*
package



*Default:*
` pkgs.python3 `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/robotframework.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/robotframework.nix)



## languages.ruby.enable



Whether to enable tools for Ruby development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/ruby.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/ruby.nix)



## languages.ruby.package



The Ruby package to use.



*Type:*
package



*Default:*
` pkgs.ruby_3_1 `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/ruby.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/ruby.nix)



## languages.ruby.bundler.enable



Whether to enable bundler.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/ruby.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/ruby.nix)



## languages.ruby.bundler.package



The bundler package to use.



*Type:*
package



*Default:*
` pkgs.bundler.override { ruby = cfg.package; } `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/ruby.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/ruby.nix)



## languages.ruby.version



The Ruby version to use.
This automatically sets the ` languages.ruby.package ` using [nixpkgs-ruby](https://github.com/bobvanderlinden/nixpkgs-ruby).



*Type:*
null or string



*Default:*
` null `



*Example:*
` "3.2.1" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/ruby.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/ruby.nix)



## languages.ruby.versionFile



The .ruby-version file path to extract the Ruby version from.
This automatically sets the ` languages.ruby.package ` using [nixpkgs-ruby](https://github.com/bobvanderlinden/nixpkgs-ruby).
When the ` .ruby-version ` file exists in the same directory as the devenv configuration, you can use:

```nix
languages.ruby.versionFile = ./.ruby-version;
```



*Type:*
null or path



*Default:*
` null `



*Example:*

```
./ruby-version

```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/ruby.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/ruby.nix)



## languages.rust.enable



Whether to enable tools for Rust development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



## languages.rust.channel



The rustup toolchain to install.



*Type:*
one of “nixpkgs”, “stable”, “beta”, “nightly”



*Default:*
` "nixpkgs" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



## languages.rust.components



List of [Rustup components](https://rust-lang.github.io/rustup/concepts/components.html)
to install. Defaults to those available in ` nixpkgs `.



*Type:*
list of string



*Default:*
` [ "rustc" "cargo" "clippy" "rustfmt" "rust-analyzer" ] `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



## languages.rust.toolchain



Rust component packages. May optionally define additional components, for example ` miri `.



*Type:*
attribute set of package



*Default:*
` nixpkgs `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



## languages.rust.toolchain.cargo



cargo package



*Type:*
null or package



*Default:*
` pkgs.cargo `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



## languages.rust.toolchain.clippy



clippy package



*Type:*
null or package



*Default:*
` pkgs.clippy `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



## languages.rust.toolchain.rust-analyzer



rust-analyzer package



*Type:*
null or package



*Default:*
` pkgs.rust-analyzer `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



## languages.rust.toolchain.rustc



rustc package



*Type:*
null or package



*Default:*
` pkgs.rustc `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



## languages.rust.toolchain.rustfmt



rustfmt package



*Type:*
null or package



*Default:*
` pkgs.rustfmt `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



## languages.scala.enable



Whether to enable tools for Scala development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/scala.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/scala.nix)



## languages.scala.package



The Scala package to use.



*Type:*
package



*Default:*
` "pkgs.scala_3" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/scala.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/scala.nix)



## languages.shell.enable



Whether to enable tools for shell development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/shell.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/shell.nix)



## languages.standardml.enable



Whether to enable tools for Standard ML development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/standardml.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/standardml.nix)



## languages.standardml.package



The Standard ML package to use.



*Type:*
package



*Default:*
` "pkgs.mlton" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/standardml.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/standardml.nix)



## languages.swift.enable



Whether to enable tools for Swift development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/swift.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/swift.nix)



## languages.swift.package



The Swift package to use.



*Type:*
package



*Default:*
` "pkgs.swift" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/swift.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/swift.nix)



## languages.terraform.enable



Whether to enable tools for Terraform development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/terraform.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/terraform.nix)



## languages.terraform.package



The Terraform package to use.



*Type:*
package



*Default:*
` pkgs.terraform `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/terraform.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/terraform.nix)



## languages.terraform.version



The Terraform version to use.
This automatically sets the ` languages.terraform.package ` using [nixpkgs-terraform](https://github.com/stackbuilders/nixpkgs-terraform).



*Type:*
null or string



*Default:*
` null `



*Example:*
` "1.5.0 or 1.6.2" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/terraform.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/terraform.nix)



## languages.texlive.enable



Whether to enable TeX Live.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/texlive.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/texlive.nix)



## languages.texlive.packages



Packages available to TeX Live



*Type:*
non-empty (list of string)



*Default:*

```
[
  "collection-basic"
]
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/texlive.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/texlive.nix)



## languages.texlive.base



TeX Live package set to use



*Type:*
unspecified value



*Default:*
` pkgs.texlive `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/texlive.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/texlive.nix)



## languages.typescript.enable



Whether to enable tools for TypeScript development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/typescript.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/typescript.nix)



## languages.unison.enable



Whether to enable tools for Unison development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/unison.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/unison.nix)



## languages.unison.package

Which package of Unison to use



*Type:*
package



*Default:*
` pkgs.unison-ucm `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/unison.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/unison.nix)



## languages.v.enable



Whether to enable tools for V development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/v.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/v.nix)



## languages.v.package



The V package to use.



*Type:*
package



*Default:*
` pkgs.vlang `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/v.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/v.nix)



## languages.vala.enable



Whether to enable tools for Vala development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/vala.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/vala.nix)



## languages.vala.package



The Vala package to use.



*Type:*
package



*Default:*
` pkgs.vala `



*Example:*
` pkgs.vala_0_54 `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/vala.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/vala.nix)



## languages.zig.enable



Whether to enable tools for Zig development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/zig.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/zig.nix)



## languages.zig.package



Which package of Zig to use.



*Type:*
package



*Default:*
` pkgs.zig `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/languages/zig.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/zig.nix)



## name



Name of the project.



*Type:*
null or string



*Default:*
` null `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/top-level.nix](https://github.com/cachix/devenv/blob/main/src/modules/top-level.nix)



## pre-commit



Integration of https://github.com/cachix/pre-commit-hooks.nix



*Type:*
submodule



*Default:*
` { } `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/pre-commit.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/pre-commit.nix)



## pre-commit.package



The ` pre-commit ` package to use.



*Type:*
package



*Default:*

```
pkgs.pre-commit

```

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit.default_stages



A configuration wide option for the stages property.
Installs hooks to the defined stages.
See [https://pre-commit.com/\#confining-hooks-to-run-at-certain-stages](https://pre-commit.com/\#confining-hooks-to-run-at-certain-stages).



*Type:*
list of string



*Default:*

```
[
  "commit"
]
```

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit.excludes



Exclude files that were matched by these patterns.



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit.hooks



The hook definitions.

You can both specify your own hooks here and you can enable predefined hooks.

Example of enabling a predefined hook:

```nix
hooks.nixpkgs-fmt.enable = true;
```

Example of a custom hook:

```nix
hooks.my-tool = {
  enable = true;
  name = "my-tool";
  description = "Run MyTool on all files in the project";
  files = "\.mtl$";
  entry = "${pkgs.my-tool}/bin/mytoolctl";
};
```

The predefined hooks are:

**` actionlint `**

Static checker for GitHub Actions workflow files.

**` alejandra `**

The Uncompromising Nix Code Formatter.

**` annex `**

Runs the git-annex hook for large file support

**` ansible-lint `**

Ansible linter.

**` autoflake `**

Remove unused imports and variables from Python code.

**` bats `**

Run bash unit tests.

**` beautysh `**

Format shell files.

**` black `**

The uncompromising Python code formatter.

**` cabal-fmt `**

Format Cabal files

**` cabal2nix `**

Run ` cabal2nix ` on all ` *.cabal ` files to generate corresponding ` default.nix ` files.

**` cargo-check `**

Check the cargo package for errors.

**` checkmake `**

Experimental linter/analyzer for Makefiles.

**` chktex `**

LaTeX semantic checker

**` clang-format `**

Format your code using ` clang-format `.

**` clang-tidy `**

Static analyzer for C++ code.

**` clippy `**

Lint Rust code.

**` cljfmt `**

A tool for formatting Clojure code.

**` cmake-format `**

A tool for formatting CMake-files.

**` commitizen `**

Check whether the current commit message follows committing rules.

**` conform `**

Policy enforcement for commits.

**` convco `**

**` credo `**

Runs a static code analysis using Credo

**` crystal `**

A tool that automatically formats Crystal source code

**` cspell `**

A Spell Checker for Code

**` deadnix `**

Scan Nix files for dead code (unused variable bindings).

**` denofmt `**

Auto-format JavaScript, TypeScript, Markdown, and JSON files.

**` denolint `**

Lint JavaScript/TypeScript source code.

**` dhall-format `**

Dhall code formatter.

**` dialyzer `**

Runs a static code analysis using Dialyzer

**` dune-fmt `**

Runs Dune’s formatters on the code tree.

**` dune-opam-sync `**

Check that Dune-generated OPAM files are in sync.

**` eclint `**

EditorConfig linter written in Go.

**` editorconfig-checker `**

Verify that the files are in harmony with the ` .editorconfig `.

**` elm-format `**

Format Elm files.

**` elm-review `**

Analyzes Elm projects, to help find mistakes before your users find them.

**` elm-test `**

Run unit tests and fuzz tests for Elm code.

**` eslint `**

Find and fix problems in your JavaScript code.

**` flake8 `**

Check the style and quality of Python files.

**` flynt `**

CLI tool to convert a python project’s %-formatted strings to f-strings.

**` fourmolu `**

Haskell code prettifier.

**` fprettify `**

Auto-formatter for modern Fortran code.

**` gofmt `**

A tool that automatically formats Go source code

**` golangci-lint `**

Fast linters runner for Go.

**` gotest `**

Run go tests

**` govet `**

Checks correctness of Go programs.

**` gptcommit `**

Generate a commit message using GPT3.

**` hadolint `**

Dockerfile linter, validate inline bash.

**` headache `**

Lightweight tool for managing headers in source code files.

**` hindent `**

Haskell code prettifier.

**` hlint `**

HLint gives suggestions on how to improve your source code.

**` hpack `**

` hpack ` converts package definitions in the hpack format (` package.yaml `) to Cabal files.

**` html-tidy `**

HTML linter.

**` hunspell `**

Spell checker and morphological analyzer.

**` isort `**

A Python utility / library to sort imports.

**` juliaformatter `**

Run JuliaFormatter.jl against Julia source files

**` latexindent `**

Perl script to add indentation to LaTeX files.

**` lua-ls `**

Uses the lua-language-server CLI to statically type-check and lint Lua code.

**` luacheck `**

A tool for linting and static analysis of Lua code.

**` lychee `**

A fast, async, stream-based link checker that finds broken hyperlinks and mail adresses inside Markdown, HTML, reStructuredText, or any other text file or website.

**` markdownlint `**

Style checker and linter for markdown files.

**` mdl `**

A tool to check markdown files and flag style issues.

**` mdsh `**

Markdown shell pre-processor.

**` mix-format `**

Runs the built-in Elixir syntax formatter

**` mix-test `**

Runs the built-in Elixir test framework

**` mkdocs-linkcheck `**

Validate links associated with markdown-based, statically generated websites.

**` mypy `**

Static type checker for Python

**` nil `**

Incremental analysis assistant for writing in Nix.

**` nixfmt `**

Nix code prettifier.

**` nixpkgs-fmt `**

Nix code prettifier.

**` ocp-indent `**

A tool to indent OCaml code.

**` opam-lint `**

OCaml package manager configuration checker.

**` ormolu `**

Haskell code prettifier.

**` php-cs-fixer `**

Lint PHP files.

**` phpcbf `**

Lint PHP files.

**` phpcs `**

Lint PHP files.

**` phpstan `**

Static Analysis of PHP files.

**` pre-commit-hook-ensure-sops `**

**` prettier `**

Opinionated multi-language code formatter.

**` psalm `**

Static Analysis of PHP files.

**` purs-tidy `**

Format purescript files.

**` purty `**

Format purescript files.

**` pylint `**

Lint Python files.

**` pyright `**

Static type checker for Python

**` pyupgrade `**

Automatically upgrade syntax for newer versions.

**` revive `**

A linter for Go source code.

**` rome `**

Unified developer tools for JavaScript, TypeScript, and the web

**` ruff `**

An extremely fast Python linter, written in Rust.

**` rustfmt `**

Format Rust code.

**` shellcheck `**

Format shell files.

**` shfmt `**

Format shell files.

**` staticcheck `**

State of the art linter for the Go programming language

**` statix `**

Lints and suggestions for the Nix programming language.

**` stylish-haskell `**

A simple Haskell code prettifier

**` stylua `**

An Opinionated Lua Code Formatter.

**` tagref `**

Have tagref check all references and tags.

**` taplo `**

Format TOML files with taplo fmt

**` terraform-format `**

Format terraform (` .tf `) files.

**` tflint `**

A Pluggable Terraform Linter.

**` topiary `**

A universal formatter engine within the Tree-sitter ecosystem, with support for many languages.

**` treefmt `**

One CLI to format the code tree.

**` typos `**

Source code spell checker

**` typstfmt `**

format typst

**` vale `**

A markup-aware linter for prose built with speed and extensibility in mind.

**` yamllint `**

Yaml linter.

**` zprint `**

Beautifully format Clojure and Clojurescript source code and s-expressions.



*Type:*
attribute set of (submodule)



*Default:*
` { } `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit.hooks.\<name>.enable



Whether to enable this pre-commit hook.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit.hooks.\<name>.always_run



if true this hook will run even if there are no matching files.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit.hooks.\<name>.description



Description of the hook. used for metadata purposes only.



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit.hooks.\<name>.entry



The entry point - the executable to run. ` entry ` can also contain arguments that will not be overridden, such as ` entry = "autopep8 -i"; `.



*Type:*
string

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit.hooks.\<name>.excludes



Exclude files that were matched by these patterns.



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit.hooks.\<name>.fail_fast



if true pre-commit will stop running hooks if this hook fails.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit.hooks.\<name>.files



The pattern of files to run on.



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit.hooks.\<name>.language



The language of the hook - tells pre-commit how to install the hook.



*Type:*
string



*Default:*
` "system" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit.hooks.\<name>.name



The name of the hook - shown during hook execution.



*Type:*
string



*Default:*
internal name, same as ` id `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit.hooks.\<name>.pass_filenames



Whether to pass filenames as arguments to the entry point.



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit.hooks.\<name>.raw



Raw fields of a pre-commit hook. This is mostly for internal use but
exposed in case you need to work around something.

Default: taken from the other hook options.



*Type:*
attribute set of unspecified value

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit.hooks.\<name>.require_serial



if true this hook will execute using a single process instead of in parallel.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit.hooks.\<name>.stages



Confines the hook to run at a particular stage.



*Type:*
list of string



*Default:*
` default_stages `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit.hooks.\<name>.types



List of file types to run on. See [Filtering files with types](https://pre-commit.com/\#plugins).



*Type:*
list of string



*Default:*

```
[
  "file"
]
```

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit.hooks.\<name>.types_or



List of file types to run on, where only a single type needs to match.



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit.hooks.\<name>.verbose



forces the output of the hook to be printed even when the hook passes.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit.installationScript



A bash snippet that installs nix-pre-commit-hooks in the current directory



*Type:*
string *(read only)*

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit.rootSrc



The source of the project to be checked.

This is used in the derivation that performs the check.

If you use the ` flakeModule `, the default is ` self.outPath `; the whole flake
sources.



*Type:*
path



*Default:*
` gitignoreSource config.src `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit.run



A derivation that tests whether the pre-commit hooks run cleanly on
the entire project.



*Type:*
package *(read only)*



*Default:*
` "<derivation>" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit.settings.alejandra.package



The ` alejandra ` package to use.



*Type:*
package



*Default:*
` "\${pkgs.alejandra}" `



*Example:*
` "\${pkgs.alejandra}" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.alejandra.check



Check if the input is already formatted and disable writing in-place the modified content



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.alejandra.exclude



Files or directories to exclude from formatting.



*Type:*
list of string



*Default:*
` [ ] `



*Example:*

```
[
  "flake.nix"
  "./templates"
]
```

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.alejandra.threads



Number of formatting threads to spawn.



*Type:*
null or signed integer



*Default:*
` null `



*Example:*
` 8 `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.alejandra.verbosity



Whether informational messages or all messages should be hidden or not.



*Type:*
one of “normal”, “quiet”, “silent”



*Default:*
` "normal" `



*Example:*
` "quiet" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.ansible-lint.configPath



Path to the YAML configuration file.



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.ansible-lint.subdir



Path to the Ansible subdirectory.



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.autoflake.binPath



Path to autoflake binary.



*Type:*
string



*Default:*

```
"${tools.autoflake}/bin/autoflake"

```

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.autoflake.flags



Flags passed to autoflake.



*Type:*
string



*Default:*
` "--in-place --expand-star-imports --remove-duplicate-keys --remove-unused-variables" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.clippy.allFeatures



Run clippy with --all-features



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.clippy.denyWarnings



Fail when warnings are present



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.clippy.offline



Run clippy offline



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.cmake-format.configPath



Path to the configuration file (.json,.python,.yaml)



*Type:*
string



*Default:*
` "" `



*Example:*
` ".cmake-format.json" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.credo.strict



Whether to auto-promote the changes.



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.deadnix.edit



Remove unused code and write to source file.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.deadnix.exclude



Files to exclude from analysis.



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.deadnix.hidden



Recurse into hidden subdirectories and process hidden .\*.nix files.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.deadnix.noLambdaArg



Don’t check lambda parameter arguments.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.deadnix.noLambdaPatternNames



Don’t check lambda pattern names (don’t break nixpkgs ` callPackage `).



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.deadnix.noUnderscore



Don’t check any bindings that start with a ` _ `.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.deadnix.quiet



Don’t print a dead code report.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.denofmt.configPath



Path to the configuration JSON file



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.denofmt.write



Whether to edit files inplace.



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.denolint.configPath



Path to the configuration JSON file



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.denolint.format



Output format.



*Type:*
one of “default”, “compact”, “json”



*Default:*
` "default" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.dune-fmt.auto-promote



Whether to auto-promote the changes.



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.dune-fmt.extraRuntimeInputs



Extra runtimeInputs to add to the environment, eg. ` ocamlformat `.



*Type:*
list of package



*Default:*
` [ ] `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.eclint.package



The ` eclint ` package to use.



*Type:*
package



*Default:*
` ${tools.eclint} `



*Example:*
` ${pkgs.eclint} `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.eclint.color



When to generate colored output.



*Type:*
one of “auto”, “always”, “never”



*Default:*
` "auto" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.eclint.exclude



Filter to exclude files.



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.eclint.fix



Modify files in place rather than showing the errors.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.eclint.summary



Only show number of errors per file.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.eclint.verbosity



Log level verbosity



*Type:*
one of 0, 1, 2, 3, 4



*Default:*
` 0 `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.eslint.binPath



` eslint ` binary path. E.g. if you want to use the ` eslint ` in ` node_modules `, use ` ./node_modules/.bin/eslint `.



*Type:*
path



*Default:*
` ${tools.eslint}/bin/eslint `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.eslint.extensions



The pattern of files to run on, see [https://pre-commit.com/\#hooks-files](https://pre-commit.com/\#hooks-files).



*Type:*
string



*Default:*
` "\.js$" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.flake8.binPath



flake8 binary path. Should be used to specify flake8 binary from your Nix-managed Python environment.



*Type:*
string



*Default:*

```
"${tools.flake8}/bin/flake8"

```

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.flake8.extendIgnore



List of additional ignore codes



*Type:*
list of string



*Default:*
` [ ] `



*Example:*

```
[
  "E501"
]
```

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.flake8.format



Output format.



*Type:*
string



*Default:*
` "default" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.flynt.package



The ` flynt ` package to use.



*Type:*
package



*Default:*
` "\${tools.flynt}" `



*Example:*
` "\${pkgs.python310Packages.flynt}" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.flynt.aggressive



Include conversions with potentially changed behavior.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.flynt.binPath



flynt binary path. Can be used to specify the flynt binary from an existing Python environment.



*Type:*
string



*Default:*
` "\${settings.flynt.package}/bin/flynt" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.flynt.dry-run



Do not change files in-place and print diff instead.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.flynt.exclude



Ignore files with given strings in their absolute path.



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.flynt.fail-on-change



Fail when diff is not empty (for linting purposes).



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.flynt.line-length



Convert expressions spanning multiple lines, only if the resulting single line will fit into this line length limit.



*Type:*
null or signed integer



*Default:*
` null `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.flynt.no-multiline



Convert only single line expressions.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.flynt.quiet



Run without output.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.flynt.string



Interpret the input as a Python code snippet and print the converted version.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.flynt.transform-concats



Replace string concatenations with f-strings.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.flynt.verbose



Run with verbose output.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.headache.header-file



Path to the header file.



*Type:*
string



*Default:*
` ".header" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.hlint.hintFile



Path to hlint.yaml. By default, hlint searches for .hlint.yaml in the project root.



*Type:*
null or path



*Default:*
` null `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.hpack.silent



Whether generation should be silent.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.isort.flags



Flags passed to isort. See all available [here](https://pycqa.github.io/isort/docs/configuration/options.html).



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.isort.profile



Built-in profiles to allow easy interoperability with common projects and code styles.



*Type:*
one of “”, “black”, “django”, “pycharm”, “google”, “open_stack”, “plone”, “attrs”, “hug”, “wemake”, “appnexus”



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.latexindent.flags



Flags passed to latexindent. See available flags [here](https://latexindentpl.readthedocs.io/en/latest/sec-how-to-use.html\#from-the-command-line)



*Type:*
string



*Default:*
` "--local --silent --overwriteIfDifferent" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.lua-ls.checklevel



The diagnostic check level



*Type:*
one of “Error”, “Warning”, “Information”, “Hint”



*Default:*
` "Warning" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.lua-ls.config



See https://github.com/LuaLS/lua-language-server/wiki/Configuration-File\#luarcjson



*Type:*
attribute set



*Default:*
` { } `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.lychee.configPath



Path to the config file.



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.lychee.flags



Flags passed to lychee. See all available [here](https://lychee.cli.rs/\#/usage/cli).



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.markdownlint.config



See https://github.com/DavidAnson/markdownlint/blob/main/schema/.markdownlint.jsonc



*Type:*
attribute set



*Default:*
` { } `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.mdl.package



The ` mdl ` package to use.



*Type:*
package



*Default:*
` "\${tools.mdl}" `



*Example:*
` "\${pkgs.mdl}" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.mdl.configPath



The configuration file to use.



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.mdl.git-recurse



Only process files known to git when given a directory.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.mdl.ignore-front-matter



Ignore YAML front matter.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.mdl.json



Format output as JSON.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.mdl.rules



Markdown rules to use for linting. Per default all rules are processed.



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.mdl.rulesets

Specify additional ruleset files to load.



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.mdl.show-aliases



Show rule alias instead of rule ID when viewing rules.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.mdl.skip-default-ruleset



Do not load the default markdownlint ruleset. Use this option if you only want to load custom rulesets.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.mdl.style



Select which style mdl uses.



*Type:*
string



*Default:*
` "default" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.mdl.tags



Markdown rules to use for linting containing the given tags. Per default all rules are processed.



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.mdl.verbose



Increase verbosity.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.mdl.warnings



Show Kramdown warnings.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.mkdocs-linkcheck.binPath



mkdocs-linkcheck binary path. Should be used to specify the mkdocs-linkcheck binary from your Nix-managed Python environment.



*Type:*
path



*Default:*

```
"${tools.mkdocs-linkcheck}/bin/mkdocs-linkcheck"

```

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.mkdocs-linkcheck.extension



File extension to scan for.



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.mkdocs-linkcheck.local-only



Whether to only check local links.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.mkdocs-linkcheck.method



HTTP method to use when checking external links.



*Type:*
one of “get”, “head”



*Default:*
` "get" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.mkdocs-linkcheck.path



Path to check



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.mkdocs-linkcheck.recurse



Whether to recurse directories under path.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.mypy.binPath



Mypy binary path. Should be used to specify the mypy executable in an environment containing your typing stubs.



*Type:*
string



*Default:*

```
"${tools.mypy}/bin/mypy"

```

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.nixfmt.width



Line width.



*Type:*
null or signed integer



*Default:*
` null `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.ormolu.cabalDefaultExtensions



Use ` default-extensions ` from ` .cabal ` files.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.ormolu.defaultExtensions



Haskell language extensions to enable.



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.php-cs-fixer.binPath



PHP-CS-Fixer binary path.



*Type:*
string



*Default:*

```
"${tools.php-cs-fixer}/bin/php-cs-fixer"

```

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.phpcbf.binPath



PHP_CodeSniffer binary path.



*Type:*
string



*Default:*

```
"${tools.phpcbf}/bin/phpcbf"

```

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.phpcs.binPath



PHP_CodeSniffer binary path.



*Type:*
string



*Default:*

```
"${tools.phpcs}/bin/phpcs"

```

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.phpstan.binPath



PHPStan binary path.



*Type:*
string



*Default:*

```
"${tools.phpstan}/bin/phpstan"

```

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.allow-parens



Include parentheses around a sole arrow function parameter.



*Type:*
one of “always”, “avoid”



*Default:*
` "always" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.binPath



` prettier ` binary path. E.g. if you want to use the ` prettier ` in ` node_modules `, use ` ./node_modules/.bin/prettier `.



*Type:*
path



*Default:*

```
"${tools.prettier}/bin/prettier"

```

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.bracket-same-line



Put > of opening tags on the last line instead of on a new line.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.cache



Only format changed files.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.cache-location



Path to the cache file location used by ` --cache ` flag.



*Type:*
string



*Default:*
` "./node_modules/.cache/prettier/.prettier-cache" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.cache-strategy



Strategy for the cache to use for detecting changed files.



*Type:*
null or one of “metadata”, “content”



*Default:*
` null `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.check



Output a human-friendly message and a list of unformatted files, if any.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.color



Colorize error messages.



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.config-precedence



Defines how config file should be evaluated in combination of CLI options.



*Type:*
one of “cli-override”, “file-override”, “prefer-file”



*Default:*
` "cli-override" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.configPath



Path to a Prettier configuration file (.prettierrc, package.json, prettier.config.js).



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.embedded-language-formatting



Control how Prettier formats quoted code embedded in the file.



*Type:*
one of “auto”, “off”



*Default:*
` "auto" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.end-of-line



Which end of line characters to apply.



*Type:*
one of “lf”, “crlf”, “cr”, “auto”



*Default:*
` "lf" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.html-whitespace-sensitivity



How to handle whitespaces in HTML.



*Type:*
one of “css”, “strict”, “ignore”



*Default:*
` "css" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.ignore-path



Path to a file containing patterns that describe files to ignore.
By default, prettier looks for ` ./.gitignore ` and ` ./.prettierignore `.
Multiple values are accepted.



*Type:*
list of path



*Default:*
` [ ] `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.ignore-unknown



Ignore unknown files.



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.insert-pragma



Insert @format pragma into file’s first docblock comment.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.jsx-single-quote



Use single quotes in JSX.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.list-different



Print the filenames of files that are different from Prettier formatting.



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.log-level



What level of logs to report.



*Type:*
one of “silent”, “error”, “warn”, “log”, “debug”



*Default:*
` "log" `



*Example:*
` "debug" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.no-bracket-spacing



Do not print spaces between brackets.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.no-config



Do not look for a configuration file.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.no-editorconfig



Don’t take .editorconfig into account when parsing configuration.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.no-error-on-unmatched-pattern



Prevent errors when pattern is unmatched.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.no-semi



Do not print semicolons, except at the beginning of lines which may need them.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.parser



Which parser to use.



*Type:*
one of “”, “flow”, “babel”, “babel-flow”, “babel-ts”, “typescript”, “acorn”, “espree”, “meriyah”, “css”, “less”, “scss”, “json”, “json5”, “json-stringify”, “graphql”, “markdown”, “mdx”, “vue”, “yaml”, “glimmer”, “html”, “angular”, “lwc”



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.plugins



Add plugins from paths.



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.print-width



Line length that the printer will wrap on.



*Type:*
signed integer



*Default:*
` 80 `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.prose-wrap



When to or if at all hard wrap prose to print width.



*Type:*
one of “always”, “never”, “preserve”



*Default:*
` "preserve" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.quote-props



Change when properties in objects are quoted.



*Type:*
one of “as-needed”, “consistent”, “preserve”



*Default:*
` "as-needed" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.require-pragma



Require either ‘@prettier’ or ‘@format’ to be present in the file’s first docblock comment.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.single-attribute-per-line



Enforce single attribute per line in HTML, Vue andJSX.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.single-quote



Number of spaces per indentation-level.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.tab-width



Line length that the printer will wrap on.



*Type:*
signed integer



*Default:*
` 2 `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.trailing-comma



Print trailing commas wherever possible in multi-line comma-separated syntactic structures.



*Type:*
one of “all”, “es5”, “none”



*Default:*
` "all" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.use-tabs



Indent with tabs instead of spaces.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.vue-indent-script-and-style



Indent script and style tags in Vue files.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.with-node-modules



Process files inside ‘node_modules’ directory.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.prettier.write



Edit files in-place.



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.psalm.binPath



Psalm binary path.



*Type:*
string



*Default:*

```
"${tools.psalm}/bin/psalm"

```

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.pylint.binPath



Pylint binary path. Should be used to specify Pylint binary from your Nix-managed Python environment.



*Type:*
string



*Default:*

```
"${tools.pylint}/bin/pylint"

```

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.pylint.reports



Whether to display a full report.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.pylint.score



Whether to activate the evaluation score.



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.pyright.binPath



Pyright binary path. Should be used to specify the pyright executable in an environment containing your typing stubs.



*Type:*
string



*Default:*

```
"${tools.pyright}/bin/pyright"

```

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.pyupgrade.binPath



pyupgrade binary path. Should be used to specify the pyupgrade binary from your Nix-managed Python environment.



*Type:*
string



*Default:*

```
"${tools.pyupgrade}/bin/pyupgrade"

```

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.revive.configPath



Path to the configuration TOML file.



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.rome.binPath



` rome ` binary path. E.g. if you want to use the ` rome ` in ` node_modules `, use ` ./node_modules/.bin/rome `.



*Type:*
path



*Default:*
` "\${tools.biome}/bin/biome" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.rome.configPath



Path to the configuration JSON file



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.rome.write



Whether to edit files inplace.



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.rust.cargoManifestPath



Path to Cargo.toml



*Type:*
null or string



*Default:*
` null `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.statix.format



Error Output format.



*Type:*
one of “stderr”, “errfmt”, “json”



*Default:*
` "errfmt" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.statix.ignore



Globs of file patterns to skip.



*Type:*
list of string



*Default:*
` [ ] `



*Example:*

```
[
  "flake.nix"
  "_*"
]
```

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.treefmt.package



The ` treefmt ` package to use.

Should include all the formatters configured by treefmt.

For example:

```nix
pkgs.writeShellApplication {
  name = "treefmt";
  runtimeInputs = [
    pkgs.treefmt
    pkgs.nixpkgs-fmt
    pkgs.black
  ];
  text =
    ''
      exec treefmt "$@"
    '';
}
```



*Type:*
package

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.typos.binary



Whether to search binary files.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.typos.color



When to use generate output.



*Type:*
one of “auto”, “always”, “never”



*Default:*
` "auto" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.typos.config



Multiline-string configuration passed as config file. If set, config set in ` typos.settings.configPath ` gets ignored.



*Type:*
string



*Default:*
` "" `



*Example:*

```
''
  [files]
  ignore-dot = true
  
  [default]
  binary = false
  
  [type.py]
  extend-glob = []
''
```

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.typos.configPath



Path to a custom config file.



*Type:*
string



*Default:*
` "" `



*Example:*
` ".typos.toml" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.typos.diff



Print a diff of what would change.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.typos.exclude



Ignore files and directories matching the glob.



*Type:*
string



*Default:*
` "" `



*Example:*
` "*.nix" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.typos.format



Output format to use.



*Type:*
one of “silent”, “brief”, “long”, “json”



*Default:*
` "long" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.typos.hidden



Search hidden files and directories.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.typos.ignored-words



Spellings and words to ignore.



*Type:*
list of string



*Default:*
` [ ] `



*Example:*

```
[
  "MQTT"
  "mosquitto"
]
```

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.typos.locale



Which language to use for spell checking.



*Type:*
one of “en”, “en-us”, “en-gb”, “en-ca”, “en-au”



*Default:*
` "en" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.typos.no-check-filenames



Skip verifying spelling in file names.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.typos.no-check-files



Skip verifying spelling in files.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.typos.no-unicode



Only allow ASCII characters in identifiers.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.typos.quiet



Less output per occurence.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.typos.verbose



More output per occurence.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.typos.write



Fix spelling in files by writing them. Cannot be used with ` typos.settings.diff `.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.vale.config



Multiline-string configuration passed as config file.



*Type:*
string



*Default:*
` "" `



*Example:*

```
''
  MinAlertLevel = suggestion
  [*]
  BasedOnStyles = Vale
''
```

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.vale.configPath



Path to the config file.



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.vale.flags



Flags passed to vale.



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.yamllint.configPath



Path to the YAML configuration file.



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.settings.yamllint.relaxed



Whether to use the relaxed configuration.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit.src



Root of the project. By default this will be filtered with the ` gitignoreSource `
function later, unless ` rootSrc ` is specified.

If you use the ` flakeModule `, the default is ` self.outPath `; the whole flake
sources.



*Type:*
path

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit.tools



Tool set from which ` nix-pre-commit-hooks ` will pick binaries.

` nix-pre-commit-hooks ` comes with its own set of packages for this purpose.



*Type:*
lazy attribute set of (null or package)



*Default:*
` pre-commit-hooks.nix-pkgs.callPackage tools-dot-nix { inherit (pkgs) system; } `

*Declared by:*
 - [https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## process.after



Bash code to execute after stopping processes.



*Type:*
strings concatenated with “\\n”



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/processes.nix](https://github.com/cachix/devenv/blob/main/src/modules/processes.nix)



## process.before



Bash code to execute before starting processes.



*Type:*
strings concatenated with “\\n”



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/processes.nix](https://github.com/cachix/devenv/blob/main/src/modules/processes.nix)



## process.implementation



The implementation used when performing ` devenv up `.



*Type:*
one of “honcho”, “overmind”, “process-compose”, “hivemind”



*Default:*
` "process-compose" `



*Example:*
` "overmind" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/processes.nix](https://github.com/cachix/devenv/blob/main/src/modules/processes.nix)



## process.process-compose



Top-level process-compose.yaml options when that implementation is used.



*Type:*
attribute set



*Default:*

```
{
  port = 9999;
  tui = true;
  version = "0.5";
}
```



*Example:*

```
{
  log_level = "fatal";
  log_location = "/path/to/combined/output/logfile.log";
  version = "0.5";
}
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/processes.nix](https://github.com/cachix/devenv/blob/main/src/modules/processes.nix)



## process-managers.hivemind.enable



Whether to enable hivemind as process-manager.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/hivemind.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/hivemind.nix)



## process-managers.hivemind.package



The hivemind package to use.



*Type:*
package



*Default:*
` pkgs.hivemind `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/hivemind.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/hivemind.nix)



## process-managers.honcho.enable



Whether to enable honcho as process-manager.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/honcho.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/honcho.nix)



## process-managers.honcho.package



The honcho package to use.



*Type:*
package



*Default:*
` pkgs.honcho `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/honcho.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/honcho.nix)



## process-managers.overmind.enable



Whether to enable overmind as process-manager.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/overmind.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/overmind.nix)



## process-managers.overmind.package



The overmind package to use.



*Type:*
package



*Default:*
` pkgs.overmind `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/overmind.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/overmind.nix)



## process-managers.process-compose.enable



Whether to enable process-compose as process-manager.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/process-compose.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/process-compose.nix)



## process-managers.process-compose.package



The process-compose package to use.



*Type:*
package



*Default:*
` pkgs.process-compose `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/process-compose.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/process-compose.nix)



## process-managers.process-compose.settings



process-compose.yaml specific process attributes.

Example: https://github.com/F1bonacc1/process-compose/blob/main/process-compose.yaml\`



*Type:*
YAML value



*Default:*
` { } `



*Example:*

```
{
  availability = {
    backoff_seconds = 2;
    max_restarts = 5;
    restart = "on_failure";
  };
  depends_on = {
    some-other-process = {
      condition = "process_completed_successfully";
    };
  };
  environment = [
    "ENVVAR_FOR_THIS_PROCESS_ONLY=foobar"
  ];
}
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/process-managers/process-compose.nix](https://github.com/cachix/devenv/blob/main/src/modules/process-managers/process-compose.nix)



## processes



Processes can be started with ` devenv up ` and run in foreground mode.



*Type:*
attribute set of (submodule)



*Default:*
` { } `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/processes.nix](https://github.com/cachix/devenv/blob/main/src/modules/processes.nix)



## processes.\<name>.exec



Bash code to run the process.



*Type:*
string

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/processes.nix](https://github.com/cachix/devenv/blob/main/src/modules/processes.nix)



## processes.\<name>.process-compose



process-compose.yaml specific process attributes.

Example: https://github.com/F1bonacc1/process-compose/blob/main/process-compose.yaml\`

Only used when using ` process.implementation = "process-compose"; `



*Type:*
attribute set



*Default:*
` { } `



*Example:*

```
{
  availability = {
    backoff_seconds = 2;
    max_restarts = 5;
    restart = "on_failure";
  };
  depends_on = {
    some-other-process = {
      condition = "process_completed_successfully";
    };
  };
  environment = [
    "ENVVAR_FOR_THIS_PROCESS_ONLY=foobar"
  ];
}
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/processes.nix](https://github.com/cachix/devenv/blob/main/src/modules/processes.nix)



## scripts



A set of scripts available when the environment is active.



*Type:*
attribute set of (submodule)



*Default:*
` { } `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/scripts.nix](https://github.com/cachix/devenv/blob/main/src/modules/scripts.nix)



## scripts.\<name>.description



Description of the script.



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/scripts.nix](https://github.com/cachix/devenv/blob/main/src/modules/scripts.nix)



## scripts.\<name>.exec



Bash code to execute when the script is run.



*Type:*
string

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/scripts.nix](https://github.com/cachix/devenv/blob/main/src/modules/scripts.nix)



## services.adminer.enable



Whether to enable Adminer process.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/adminer.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/adminer.nix)



## services.adminer.package



Which package of Adminer to use.



*Type:*
package



*Default:*
` pkgs.adminer `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/adminer.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/adminer.nix)



## services.adminer.listen



Listen address for the Adminer.



*Type:*
string



*Default:*
` "127.0.0.1:8080" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/adminer.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/adminer.nix)



## services.blackfire.enable



Whether to enable Blackfire profiler agent

It automatically installs Blackfire PHP extension.
.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/blackfire.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/blackfire.nix)



## services.blackfire.enableApm



Whether to enable Enables application performance monitoring, requires special subscription.
.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/blackfire.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/blackfire.nix)



## services.blackfire.package



Which package of blackfire to use



*Type:*
package



*Default:*
` pkgs.blackfire `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/blackfire.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/blackfire.nix)



## services.blackfire.client-id



Sets the client id used to authenticate with Blackfire.
You can find your personal client-id at [https://blackfire.io/my/settings/credentials](https://blackfire.io/my/settings/credentials).



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/blackfire.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/blackfire.nix)



## services.blackfire.client-token



Sets the client token used to authenticate with Blackfire.
You can find your personal client-token at [https://blackfire.io/my/settings/credentials](https://blackfire.io/my/settings/credentials).



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/blackfire.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/blackfire.nix)



## services.blackfire.server-id



Sets the server id used to authenticate with Blackfire.
You can find your personal server-id at [https://blackfire.io/my/settings/credentials](https://blackfire.io/my/settings/credentials).



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/blackfire.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/blackfire.nix)



## services.blackfire.server-token



Sets the server token used to authenticate with Blackfire.
You can find your personal server-token at [https://blackfire.io/my/settings/credentials](https://blackfire.io/my/settings/credentials).



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/blackfire.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/blackfire.nix)



## services.blackfire.socket



Sets the server socket path



*Type:*
string



*Default:*
` "tcp://127.0.0.1:8307" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/blackfire.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/blackfire.nix)



## services.caddy.enable



Whether to enable Caddy web server.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix)



## services.caddy.package



Caddy package to use.



*Type:*
package



*Default:*
` pkgs.caddy `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix)



## services.caddy.adapter



Name of the config adapter to use.
See [https://caddyserver.com/docs/config-adapters](https://caddyserver.com/docs/config-adapters) for the full list.



*Type:*
string



*Default:*
` "caddyfile" `



*Example:*
` "nginx" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix)



## services.caddy.ca



Certificate authority ACME server. The default (Let’s Encrypt
production server) should be fine for most people. Set it to null if
you don’t want to include any authority (or if you want to write a more
fine-graned configuration manually).



*Type:*
null or string



*Default:*
` "https://acme-v02.api.letsencrypt.org/directory" `



*Example:*
` "https://acme-staging-v02.api.letsencrypt.org/directory" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix)



## services.caddy.config



Verbatim Caddyfile to use.
Caddy v2 supports multiple config formats via adapters (see [` services.caddy.adapter `](\#servicescaddyconfig)).



*Type:*
strings concatenated with “\\n”



*Default:*
` "" `



*Example:*

```
''
  example.com {
    encode gzip
    log
    root /srv/http
  }
''
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix)



## services.caddy.dataDir



The data directory, for storing certificates. Before 17.09, this
would create a .caddy directory. With 17.09 the contents of the
.caddy directory are in the specified data directory instead.
Caddy v2 replaced CADDYPATH with XDG directories.
See [https://caddyserver.com/docs/conventions\#file-locations](https://caddyserver.com/docs/conventions\#file-locations).



*Type:*
path



*Default:*
` "/home/runner/work/devenv/devenv/.devenv/state/caddy" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix)



## services.caddy.email



Email address (for Let’s Encrypt certificate).



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix)



## services.caddy.resume



Use saved config, if any (and prefer over configuration passed with [` caddy.config `](\#caddyconfig)).



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix)



## services.caddy.virtualHosts



Declarative vhost config.



*Type:*
attribute set of (submodule)



*Default:*
` { } `



*Example:*

```
{
  "hydra.example.com" = {
    serverAliases = [ "www.hydra.example.com" ];
    extraConfig = ''''
      encode gzip
      log
      root /srv/http
    '''';
  };
};

```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix)



## services.caddy.virtualHosts.\<name>.extraConfig



These lines go into the vhost verbatim.



*Type:*
strings concatenated with “\\n”



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix)



## services.caddy.virtualHosts.\<name>.serverAliases



Additional names of virtual hosts served by this virtual host configuration.



*Type:*
list of string



*Default:*
` [ ] `



*Example:*

```
[
  "www.example.org"
  "example.org"
]
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix)



## services.cassandra.enable



Whether to enable Add Cassandra process script…



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/cassandra.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/cassandra.nix)



## services.cassandra.package



Which version of Cassandra to use



*Type:*
package



*Default:*
` pkgs.cassandra_4 `



*Example:*
` pkgs.cassandra_4; `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/cassandra.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/cassandra.nix)



## services.cassandra.allowClients



Enables or disables the native transport server (CQL binary protocol)



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/cassandra.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/cassandra.nix)



## services.cassandra.clusterName



The name of the cluster



*Type:*
string



*Default:*
` "Test Cluster" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/cassandra.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/cassandra.nix)



## services.cassandra.extraConfig



Extra options to be merged into ` cassandra.yaml ` as nix attribute set.



*Type:*
attribute set



*Default:*
` { } `



*Example:*

```
{
  commitlog_sync_batch_window_in_ms = 3;
}
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/cassandra.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/cassandra.nix)



## services.cassandra.jvmOpts



Options to pass to the JVM through the JVM_OPTS environment variable



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/cassandra.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/cassandra.nix)



## services.cassandra.listenAddress



Listen address



*Type:*
string



*Default:*
` "127.0.0.1" `



*Example:*
` "127.0.0.1" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/cassandra.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/cassandra.nix)



## services.cassandra.seedAddresses



The addresses of hosts designated as contact points of the cluster



*Type:*
list of string



*Default:*

```
[
  "127.0.0.1"
]
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/cassandra.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/cassandra.nix)



## services.clickhouse.enable



Whether to enable clickhouse-server.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/clickhouse.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/clickhouse.nix)



## services.clickhouse.package



Which package of clickhouse to use



*Type:*
package



*Default:*
` pkgs.clickhouse `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/clickhouse.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/clickhouse.nix)



## services.clickhouse.config



ClickHouse configuration in YAML.



*Type:*
strings concatenated with “\\n”

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/clickhouse.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/clickhouse.nix)



## services.clickhouse.port



Which port to run clickhouse on



*Type:*
signed integer



*Default:*
` 9000 `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/clickhouse.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/clickhouse.nix)



## services.cockroachdb.enable



Whether to enable Add CockroachDB process.
.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/cockroachdb.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/cockroachdb.nix)



## services.cockroachdb.package



The CockroachDB package to use.



*Type:*
unspecified value



*Default:*
` "pkgs.cockroachdb-bin" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/cockroachdb.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/cockroachdb.nix)



## services.cockroachdb.http_addr



The hostname or IP address to bind to for HTTP requests.



*Type:*
string



*Default:*
` "localhost:8080" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/cockroachdb.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/cockroachdb.nix)



## services.cockroachdb.listen_addr



The address/hostname and port to listen on.



*Type:*
string



*Default:*
` "localhost:26257" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/cockroachdb.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/cockroachdb.nix)



## services.couchdb.enable



Whether to enable CouchDB process.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/couchdb.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/couchdb.nix)



## services.couchdb.package



Which version of CouchDB to use



*Type:*
package



*Default:*
` pkgs.couchdb3 `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/couchdb.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/couchdb.nix)



## services.couchdb.settings



CouchDB configuration.
to know more about all settings, look at:
\<link
xlink:href=“https://docs.couchdb.org/en/stable/config/couchdb.html”
/>



*Type:*
attribute set of section of an INI file (attrs of INI atom (null, bool, int, float or string))



*Default:*
` { } `



*Example:*

```
{
  couchdb = {
    database_dir = baseDir;
    single_node = true;
    view_index_dir = baseDir;
    uri_file = "/home/runner/work/devenv/devenv/.devenv/state/couchdb/couchdb.uri";
  };
  admins = {
    "admin_username" = "pass";
  };
  chttpd = {
    bind_address = "127.0.0.1";
    port = 5984;
  };
}

```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/couchdb.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/couchdb.nix)



## services.couchdb.settings.chttpd.bind_address



Defines the IP address by which CouchDB will be accessible.



*Type:*
string



*Default:*
` "127.0.0.1" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/couchdb.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/couchdb.nix)



## services.couchdb.settings.chttpd.port



Defined the port number to listen.



*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*
` 5984 `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/couchdb.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/couchdb.nix)



## services.couchdb.settings.couchdb.database_dir



Specifies location of CouchDB database files (\*.couch named). This
location should be writable and readable for the user the CouchDB
service runs as (couchdb by default).



*Type:*
path



*Default:*
` "/home/runner/work/devenv/devenv/.devenv/state/couchdb" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/couchdb.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/couchdb.nix)



## services.couchdb.settings.couchdb.single_node



When this configuration setting is set to true, automatically create
the system databases on startup. Must be set false for a clustered
CouchDB installation.



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/couchdb.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/couchdb.nix)



## services.couchdb.settings.couchdb.uri_file



This file contains the full URI that can be used to access this
instance of CouchDB. It is used to help discover the port CouchDB is
running on (if it was set to 0 (e.g. automatically assigned any free
one). This file should be writable and readable for the user that
runs the CouchDB service (couchdb by default).



*Type:*
path



*Default:*
` "/home/runner/work/devenv/devenv/.devenv/state/couchdb/couchdb.uri" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/couchdb.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/couchdb.nix)



## services.couchdb.settings.couchdb.view_index_dir



Specifies location of CouchDB view index files. This location should
be writable and readable for the user that runs the CouchDB service
(couchdb by default).



*Type:*
path



*Default:*
` "/home/runner/work/devenv/devenv/.devenv/state/couchdb" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/couchdb.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/couchdb.nix)



## services.dynamodb-local.enable



Whether to enable DynamoDB Local.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/dynamodb-local.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/dynamodb-local.nix)



## services.dynamodb-local.package



Which package of DynamoDB to use.



*Type:*
package



*Default:*
` pkgs.dynamodb-local `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/dynamodb-local.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/dynamodb-local.nix)



## services.dynamodb-local.port



Listen address for the Dynamodb-local.



*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*
` 8000 `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/dynamodb-local.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/dynamodb-local.nix)



## services.elasticmq.enable



Whether to enable elasticmq-server.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/elasticmq.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticmq.nix)



## services.elasticmq.package



Which package of elasticmq-server-bin to use



*Type:*
package



*Default:*
` pkgs.elasticmq-server-bin `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/elasticmq.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticmq.nix)



## services.elasticmq.settings



Configuration for elasticmq-server



*Type:*
strings concatenated with “\\n”



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/elasticmq.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticmq.nix)



## services.elasticsearch.enable



Whether to enable elasticsearch.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix)



## services.elasticsearch.package



Elasticsearch package to use.



*Type:*
package



*Default:*
` pkgs.elasticsearch7 `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix)



## services.elasticsearch.cluster_name



Elasticsearch name that identifies your cluster for auto-discovery.



*Type:*
string



*Default:*
` "elasticsearch" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix)



## services.elasticsearch.extraCmdLineOptions



Extra command line options for the elasticsearch launcher.



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix)



## services.elasticsearch.extraConf



Extra configuration for elasticsearch.



*Type:*
string



*Default:*
` "" `



*Example:*

```
''
  node.name: "elasticsearch"
  node.master: true
  node.data: false
''
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix)



## services.elasticsearch.extraJavaOptions



Extra command line options for Java.



*Type:*
list of string



*Default:*
` [ ] `



*Example:*

```
[
  "-Djava.net.preferIPv4Stack=true"
]
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix)



## services.elasticsearch.listenAddress



Elasticsearch listen address.



*Type:*
string



*Default:*
` "127.0.0.1" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix)



## services.elasticsearch.logging



Elasticsearch logging configuration.



*Type:*
string



*Default:*

```
''
  logger.action.name = org.elasticsearch.action
  logger.action.level = info
  appender.console.type = Console
  appender.console.name = console
  appender.console.layout.type = PatternLayout
  appender.console.layout.pattern = [%d{ISO8601}][%-5p][%-25c{1.}] %marker%m%n
  rootLogger.level = info
  rootLogger.appenderRef.console.ref = console
''
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix)



## services.elasticsearch.plugins



Extra elasticsearch plugins



*Type:*
list of package



*Default:*
` [ ] `



*Example:*
` [ pkgs.elasticsearchPlugins.discovery-ec2 ] `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix)



## services.elasticsearch.port



Elasticsearch port to listen for HTTP traffic.



*Type:*
signed integer



*Default:*
` 9200 `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix)



## services.elasticsearch.single_node



Start a single-node cluster



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix)



## services.elasticsearch.tcp_port



Elasticsearch port for the node to node communication.



*Type:*
signed integer



*Default:*
` 9300 `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix)



## services.influxdb.enable



Whether to enable influxdb.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/influxdb.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/influxdb.nix)



## services.influxdb.package



An open-source distributed time series database



*Type:*
package



*Default:*
` pkgs.influxdb `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/influxdb.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/influxdb.nix)



## services.influxdb.config



Configuration for InfluxDB-server



*Type:*
strings concatenated with “\\n”



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/influxdb.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/influxdb.nix)



## services.mailhog.enable



Whether to enable mailhog process.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mailhog.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mailhog.nix)



## services.mailhog.package



Which package of mailhog to use



*Type:*
package



*Default:*
` pkgs.mailhog `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mailhog.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mailhog.nix)



## services.mailhog.additionalArgs



Additional arguments passed to ` mailhog `.



*Type:*
list of strings concatenated with “\\n”



*Default:*
` [ ] `



*Example:*

```
[
  "-invite-jim"
]
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mailhog.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mailhog.nix)



## services.mailhog.apiListenAddress



Listen address for API.



*Type:*
string



*Default:*
` "127.0.0.1:8025" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mailhog.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mailhog.nix)



## services.mailhog.smtpListenAddress



Listen address for SMTP.



*Type:*
string



*Default:*
` "127.0.0.1:1025" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mailhog.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mailhog.nix)



## services.mailhog.uiListenAddress



Listen address for UI.



*Type:*
string



*Default:*
` "127.0.0.1:8025" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mailhog.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mailhog.nix)



## services.mailpit.enable



Whether to enable mailpit process.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mailpit.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mailpit.nix)



## services.mailpit.package



Which package of mailpit to use



*Type:*
package



*Default:*
` pkgs.mailpit `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mailpit.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mailpit.nix)



## services.mailpit.additionalArgs



Additional arguments passed to ` mailpit `.



*Type:*
list of strings concatenated with “\\n”



*Default:*
` [ ] `



*Example:*

```
[
  "--max=500"
]
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mailpit.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mailpit.nix)



## services.mailpit.smtpListenAddress



Listen address for SMTP.



*Type:*
string



*Default:*
` "127.0.0.1:1025" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mailpit.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mailpit.nix)



## services.mailpit.uiListenAddress



Listen address for UI.



*Type:*
string



*Default:*
` "127.0.0.1:8025" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mailpit.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mailpit.nix)



## services.meilisearch.enable



Whether to enable Meilisearch.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/meilisearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/meilisearch.nix)



## services.meilisearch.environment



Defines the running environment of Meilisearch.



*Type:*
one of “development”, “production”



*Default:*
` "development" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/meilisearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/meilisearch.nix)



## services.meilisearch.listenAddress



Meilisearch listen address.



*Type:*
string



*Default:*
` "127.0.0.1" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/meilisearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/meilisearch.nix)



## services.meilisearch.listenPort



Meilisearch port to listen on.



*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*
` 7700 `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/meilisearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/meilisearch.nix)



## services.meilisearch.logLevel



Defines how much detail should be present in Meilisearch’s logs.
Meilisearch currently supports four log levels, listed in order of increasing verbosity:

 - ‘ERROR’: only log unexpected events indicating Meilisearch is not functioning as expected
 - ‘WARN:’ log all unexpected events, regardless of their severity
 - ‘INFO:’ log all events. This is the default value
 - ‘DEBUG’: log all events and including detailed information on Meilisearch’s internal processes.
   Useful when diagnosing issues and debugging



*Type:*
string



*Default:*
` "INFO" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/meilisearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/meilisearch.nix)



## services.meilisearch.maxIndexSize



Sets the maximum size of the index.
Value must be given in bytes or explicitly stating a base unit.
For example, the default value can be written as 107374182400, ‘107.7Gb’, or ‘107374 Mb’.
Default is 100 GiB



*Type:*
string



*Default:*
` "107374182400" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/meilisearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/meilisearch.nix)



## services.meilisearch.noAnalytics



Deactivates analytics.
Analytics allow Meilisearch to know how many users are using Meilisearch,
which versions and which platforms are used.
This process is entirely anonymous.



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/meilisearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/meilisearch.nix)



## services.memcached.enable



Whether to enable memcached process.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/memcached.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/memcached.nix)



## services.memcached.package



Which package of memcached to use



*Type:*
package



*Default:*
` pkgs.memcached `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/memcached.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/memcached.nix)



## services.memcached.bind



The IP interface to bind to.
` null ` means “all interfaces”.



*Type:*
null or string



*Default:*
` "127.0.0.1" `



*Example:*
` "127.0.0.1" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/memcached.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/memcached.nix)



## services.memcached.port



The TCP port to accept connections.
If port 0 is specified Redis will not listen on a TCP socket.



*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*
` 11211 `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/memcached.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/memcached.nix)



## services.memcached.startArgs



Additional arguments passed to ` memcached ` during startup.



*Type:*
list of strings concatenated with “\\n”



*Default:*
` [ ] `



*Example:*

```
[
  "--memory-limit=100M"
]
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/memcached.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/memcached.nix)



## services.minio.enable



Whether to enable MinIO Object Storage.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix)



## services.minio.package



MinIO package to use.



*Type:*
package



*Default:*
` pkgs.minio `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix)



## services.minio.accessKey



Access key of 5 to 20 characters in length that clients use to access the server.



*Type:*
string



*Default:*
` "minioadmin" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix)



## services.minio.afterStart



Bash code to execute after minio is running.



*Type:*
strings concatenated with “\\n”



*Default:*
` "" `



*Example:*

```
''
  mc anonymous set download local/mybucket
''
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix)



## services.minio.browser



Enable or disable access to web UI.



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix)



## services.minio.buckets



List of buckets to ensure exist on startup.



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix)



## services.minio.clientConfig



Contents of the mc ` config.json `, as a nix attribute set.

By default, ` local ` is configured to connect to the devenv minio service.
Use ` lib.mkForce null ` to use your regular mc configuration from ` $HOME/.mc ` instead.



*Type:*
null or JSON value

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix)



## services.minio.clientPackage



MinIO client package to use.



*Type:*
package



*Default:*
` pkgs.minio-client `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix)



## services.minio.consoleAddress



IP address and port of the web UI (console).



*Type:*
string



*Default:*
` "127.0.0.1:9001" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix)



## services.minio.listenAddress



IP address and port of the server.



*Type:*
string



*Default:*
` "127.0.0.1:9000" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix)



## services.minio.region



The physical location of the server. By default it is set to us-east-1, which is same as AWS S3’s and MinIO’s default region.



*Type:*
string



*Default:*
` "us-east-1" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix)



## services.minio.secretKey



Specify the Secret key of 8 to 40 characters in length that clients use to access the server.



*Type:*
string



*Default:*
` "minioadmin" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix)



## services.mongodb.enable



Whether to enable MongoDB process and expose utilities.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mongodb.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mongodb.nix)



## services.mongodb.package



Which MongoDB package to use.



*Type:*
package



*Default:*
` pkgs.mongodb `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mongodb.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mongodb.nix)



## services.mongodb.additionalArgs



Additional arguments passed to ` mongod `.



*Type:*
list of strings concatenated with “\\n”



*Default:*

```
[
  "--noauth"
]
```



*Example:*

```
[
  "--port"
  "27017"
  "--noauth"
]
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mongodb.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mongodb.nix)



## services.mongodb.initDatabasePassword



This used in conjunction with initDatabaseUsername, create a new user and set that user’s password. This user is created in the admin authentication database and given the role of root, which is a “superuser” role.



*Type:*
string



*Default:*
` "" `



*Example:*
` "secret" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mongodb.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mongodb.nix)



## services.mongodb.initDatabaseUsername



This used in conjunction with initDatabasePassword, create a new user and set that user’s password. This user is created in the admin authentication database and given the role of root, which is a “superuser” role.



*Type:*
string



*Default:*
` "" `



*Example:*
` "mongoadmin" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mongodb.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mongodb.nix)



## services.mysql.enable



Whether to enable MySQL process and expose utilities.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix)



## services.mysql.package



Which package of MySQL to use



*Type:*
package



*Default:*
` pkgs.mariadb `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix)



## services.mysql.ensureUsers



Ensures that the specified users exist and have at least the ensured permissions.
The MySQL users will be identified using Unix socket authentication. This authenticates the Unix user with the
same name only, and that without the need for a password.
This option will never delete existing users or remove permissions, especially not when the value of this
option is changed. This means that users created and permissions assigned once through this option or
otherwise have to be removed manually.



*Type:*
list of (submodule)



*Default:*
` [ ] `



*Example:*

```
[
  {
    name = "devenv";
    ensurePermissions = {
      "devenv.*" = "ALL PRIVILEGES";
    };
  }
]

```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix)



## services.mysql.ensureUsers.\*.ensurePermissions



Permissions to ensure for the user, specified as attribute set.
The attribute names specify the database and tables to grant the permissions for,
separated by a dot. You may use wildcards here.
The attribute values specfiy the permissions to grant.
You may specify one or multiple comma-separated SQL privileges here.
For more information on how to specify the target
and on which privileges exist, see the
[GRANT syntax](https://mariadb.com/kb/en/library/grant/).
The attributes are used as ` GRANT ${attrName} ON ${attrValue} `.



*Type:*
attribute set of string



*Default:*
` { } `



*Example:*

```
{
  "database.*" = "ALL PRIVILEGES";
  "*.*" = "SELECT, LOCK TABLES";
}

```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix)



## services.mysql.ensureUsers.\*.name



Name of the user to ensure.



*Type:*
string

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix)



## services.mysql.ensureUsers.\*.password



Password of the user to ensure.



*Type:*
null or string



*Default:*
` null `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix)



## services.mysql.importTimeZones



Whether to import tzdata on the first startup of the mysql server



*Type:*
null or boolean



*Default:*
` null `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix)



## services.mysql.initialDatabases



List of database names and their initial schemas that should be used to create databases on the first startup
of MySQL. The schema attribute is optional: If not specified, an empty database is created.



*Type:*
list of (submodule)



*Default:*
` [ ] `



*Example:*

```
[
  { name = "foodatabase"; schema = ./foodatabase.sql; }
  { name = "bardatabase"; }
]

```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix)



## services.mysql.initialDatabases.\*.name



The name of the database to create.



*Type:*
string

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix)



## services.mysql.initialDatabases.\*.schema



The initial schema of the database; if null (the default),
an empty database is created.



*Type:*
null or path



*Default:*
` null `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix)



## services.mysql.settings



MySQL configuration.



*Type:*
lazy attribute set of lazy attribute set of anything



*Default:*
` { } `



*Example:*

```
{
  mysqld = {
    key_buffer_size = "6G";
    table_cache = 1600;
    log-error = "/var/log/mysql_err.log";
    plugin-load-add = [ "server_audit" "ed25519=auth_ed25519" ];
  };
  mysqldump = {
    quick = true;
    max_allowed_packet = "16M";
  };
}

```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix)



## services.mysql.useDefaultsExtraFile



Whether to use defaults-exta-file for the mysql command instead of defaults-file.
This is useful if you want to provide a config file on the command line.
However this can problematic if you have MySQL installed globaly because its config might leak into your environment.
This option does not affect the mysqld command.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix)



## services.nginx.enable



Whether to enable nginx.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/nginx.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/nginx.nix)



## services.nginx.package



The nginx package to use.



*Type:*
package



*Default:*
` "pkgs.nginx" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/nginx.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/nginx.nix)



## services.nginx.defaultMimeTypes



Default MIME types for NGINX, as MIME types definitions from NGINX are very incomplete,
we use by default the ones bundled in the mailcap package, used by most of the other
Linux distributions.



*Type:*
path



*Default:*
` $''{pkgs.mailcap}/etc/nginx/mime.types `



*Example:*
` $''{pkgs.nginx}/conf/mime.types `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/nginx.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/nginx.nix)



## services.nginx.eventsConfig



The nginx events configuration.



*Type:*
strings concatenated with “\\n”



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/nginx.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/nginx.nix)



## services.nginx.httpConfig



The nginx configuration.



*Type:*
strings concatenated with “\\n”



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/nginx.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/nginx.nix)



## services.opensearch.enable



Whether to enable OpenSearch.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/opensearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/opensearch.nix)



## services.opensearch.package



The OpenSearch package to use.



*Type:*
package



*Default:*
` pkgs.opensearch `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/opensearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/opensearch.nix)



## services.opensearch.extraCmdLineOptions



Extra command line options for the OpenSearch launcher.



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/opensearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/opensearch.nix)



## services.opensearch.extraJavaOptions



Extra command line options for Java.



*Type:*
list of string



*Default:*
` [ ] `



*Example:*

```
[
  "-Djava.net.preferIPv4Stack=true"
]
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/opensearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/opensearch.nix)



## services.opensearch.logging



OpenSearch logging configuration.



*Type:*
string



*Default:*

```
''
  logger.action.name = org.opensearch.action
  logger.action.level = info
  appender.console.type = Console
  appender.console.name = console
  appender.console.layout.type = PatternLayout
  appender.console.layout.pattern = [%d{ISO8601}][%-5p][%-25c{1.}] %marker%m%n
  rootLogger.level = info
  rootLogger.appenderRef.console.ref = console
''
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/opensearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/opensearch.nix)



## services.opensearch.settings



OpenSearch configuration.



*Type:*
YAML value



*Default:*
` { } `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/opensearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/opensearch.nix)



## services.opensearch.settings."cluster.name"



The name of the cluster.



*Type:*
string



*Default:*
` "opensearch" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/opensearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/opensearch.nix)



## services.opensearch.settings."discovery.type"



The type of discovery to use.



*Type:*
string



*Default:*
` "single-node" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/opensearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/opensearch.nix)



## services.opensearch.settings."http.port"



The port to listen on for HTTP traffic.



*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*
` 9200 `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/opensearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/opensearch.nix)



## services.opensearch.settings."network.host"



Which port this service should listen on.



*Type:*
string



*Default:*
` "127.0.0.1" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/opensearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/opensearch.nix)



## services.opensearch.settings."transport.port"



The port to listen on for transport traffic.



*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*
` 9300 `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/opensearch.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/opensearch.nix)



## services.postgres.enable



Whether to enable Add PostgreSQL process.
.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix)



## services.postgres.package



The PostgreSQL package to use. Use this to override the default with a specific version.



*Type:*
package



*Default:*
` pkgs.postgresql `



*Example:*

```
pkgs.postgresql_15

```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix)



## services.postgres.createDatabase



Create a database named like current user on startup. Only applies when initialDatabases is an empty list.



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix)



## services.postgres.extensions



Additional PostgreSQL extensions to install.

The available extensions are:

 - age
 - apache_datasketches
 - citus
 - cstore_fdw
 - h3-pg
 - hypopg
 - jsonb_deep_sum
 - lantern
 - periods
 - pg_auto_failover
 - pg_bigm
 - pg_cron
 - pg_ed25519
 - pg_embedding
 - pg_hint_plan
 - pg_hll
 - pg_ivm
 - pg_net
 - pg_partman
 - pg_rational
 - pg_relusage
 - pg_repack
 - pg_safeupdate
 - pg_similarity
 - pg_squeeze
 - pg_topn
 - pg_uuidv7
 - pgaudit
 - pgjwt
 - pgroonga
 - pgrouting
 - pgsodium
 - pgsql-http
 - pgtap
 - pgvecto-rs
 - pgvector
 - plpgsql_check
 - plr
 - plv8
 - postgis
 - promscale_extension
 - repmgr
 - rum
 - smlar
 - tds_fdw
 - temporal_tables
 - timescaledb
 - timescaledb-apache
 - timescaledb_toolkit
 - tsearch_extras
 - tsja
 - wal2json



*Type:*
null or (function that evaluates to a(n) list of package)



*Default:*
` null `



*Example:*

```
extensions: [
  extensions.pg_cron
  extensions.postgis
  extensions.timescaledb
];

```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix)



## services.postgres.initdbArgs



Additional arguments passed to ` initdb ` during data dir
initialisation.



*Type:*
list of strings concatenated with “\\n”



*Default:*

```
[
  "--locale=C"
  "--encoding=UTF8"
]
```



*Example:*

```
[
  "--data-checksums"
  "--allow-group-access"
]
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix)



## services.postgres.initialDatabases



List of database names and their initial schemas that should be used to create databases on the first startup
of Postgres. The schema attribute is optional: If not specified, an empty database is created.



*Type:*
list of (submodule)



*Default:*
` [ ] `



*Example:*

```
[
  {
    name = "foodatabase";
    schema = ./foodatabase.sql;
  }
  { name = "bardatabase"; }
]

```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix)



## services.postgres.initialDatabases.\*.name



The name of the database to create.



*Type:*
string

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix)



## services.postgres.initialDatabases.\*.schema



The initial schema of the database; if null (the default),
an empty database is created.



*Type:*
null or path



*Default:*
` null `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix)



## services.postgres.initialScript



Initial SQL commands to run during database initialization. This can be multiple
SQL expressions separated by a semi-colon.



*Type:*
null or string



*Default:*
` null `



*Example:*

```
CREATE ROLE postgres SUPERUSER;
CREATE ROLE bar;

```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix)



## services.postgres.listen_addresses



Listen address



*Type:*
string



*Default:*
` "" `



*Example:*
` "127.0.0.1" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix)



## services.postgres.port



The TCP port to accept connections.



*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*
` 5432 `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix)



## services.postgres.settings



PostgreSQL configuration. Refer to
[https://www.postgresql.org/docs/11/config-setting.html\#CONFIG-SETTING-CONFIGURATION-FILE](https://www.postgresql.org/docs/11/config-setting.html\#CONFIG-SETTING-CONFIGURATION-FILE)
for an overview of ` postgresql.conf `.

String values will automatically be enclosed in single quotes. Single quotes will be
escaped with two single quotes as described by the upstream documentation linked above.



*Type:*
attribute set of (boolean or floating point number or signed integer or string)



*Default:*
` { } `



*Example:*

```
{
  log_connections = true;
  log_statement = "all";
  logging_collector = true
  log_disconnections = true
  log_destination = lib.mkForce "syslog";
}

```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix)



## services.rabbitmq.enable



Whether to enable the RabbitMQ server, an Advanced Message
Queuing Protocol (AMQP) broker.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix)



## services.rabbitmq.package



Which rabbitmq package to use.



*Type:*
package



*Default:*
` pkgs.rabbitmq-server `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix)



## services.rabbitmq.configItems



Configuration options in RabbitMQ’s new config file format,
which is a simple key-value format that can not express nested
data structures. This is known as the ` rabbitmq.conf ` file,
although outside NixOS that filename may have Erlang syntax, particularly
prior to RabbitMQ 3.7.0.
If you do need to express nested data structures, you can use
` config ` option. Configuration from ` config `
will be merged into these options by RabbitMQ at runtime to
form the final configuration.
See [https://www.rabbitmq.com/configure.html\#config-items](https://www.rabbitmq.com/configure.html\#config-items)
For the distinct formats, see [https://www.rabbitmq.com/configure.html\#config-file-formats](https://www.rabbitmq.com/configure.html\#config-file-formats)



*Type:*
attribute set of string



*Default:*
` { } `



*Example:*

```
{
  "auth_backends.1.authn" = "rabbit_auth_backend_ldap";
  "auth_backends.1.authz" = "rabbit_auth_backend_internal";
}

```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix)



## services.rabbitmq.cookie



Erlang cookie is a string of arbitrary length which must
be the same for several nodes to be allowed to communicate.
Leave empty to generate automatically.



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix)



## services.rabbitmq.listenAddress



IP address on which RabbitMQ will listen for AMQP
connections.  Set to the empty string to listen on all
interfaces.  Note that RabbitMQ creates a user named
` guest ` with password
` guest ` by default, so you should delete
this user if you intend to allow external access.
Together with ‘port’ setting it’s mostly an alias for
configItems.“listeners.tcp.1” and it’s left for backwards
compatibility with previous version of this module.



*Type:*
string



*Default:*
` "127.0.0.1" `



*Example:*
` "" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix)



## services.rabbitmq.managementPlugin.enable



Whether to enable the management plugin.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix)



## services.rabbitmq.managementPlugin.port



On which port to run the management plugin



*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*
` 15672 `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix)



## services.rabbitmq.nodeName



The name of the RabbitMQ node.  This is used to identify
the node in a cluster.  If you are running multiple
RabbitMQ nodes on the same machine, you must give each
node a unique name.  The name must be of the form
` name@host `, where ` name ` is an arbitrary name and
` host ` is the domain name of the host.



*Type:*
string



*Default:*
` "rabbit@localhost" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix)



## services.rabbitmq.pluginDirs



The list of directories containing external plugins



*Type:*
list of path



*Default:*
` [ ] `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix)



## services.rabbitmq.plugins



The names of plugins to enable



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix)



## services.rabbitmq.port



Port on which RabbitMQ will listen for AMQP connections.



*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*
` 5672 `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix)



## services.redis.enable



Whether to enable Redis process and expose utilities.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/redis.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/redis.nix)



## services.redis.package



Which package of Redis to use



*Type:*
package



*Default:*
` pkgs.redis `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/redis.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/redis.nix)



## services.redis.bind



The IP interface to bind to.
` null ` means “all interfaces”.



*Type:*
null or string



*Default:*
` "127.0.0.1" `



*Example:*
` "127.0.0.1" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/redis.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/redis.nix)



## services.redis.extraConfig



Additional text to be appended to ` redis.conf `.



*Type:*
strings concatenated with “\\n”



*Default:*
` "locale-collate C" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/redis.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/redis.nix)



## services.redis.port



The TCP port to accept connections.
If port 0 is specified Redis, will not listen on a TCP socket.



*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*
` 6379 `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/redis.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/redis.nix)



## services.temporal.enable



Whether to enable Temporal process.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/temporal.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/temporal.nix)



## services.temporal.package



Which package of Temporal to use.



*Type:*
package



*Default:*
` pkgs.temporal-cli `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/temporal.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/temporal.nix)



## services.temporal.ip



IPv4 address to bind the frontend service to.



*Type:*
string



*Default:*
` "127.0.0.1" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/temporal.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/temporal.nix)



## services.temporal.namespaces



Specify namespaces that should be pre-created (namespace “default” is always created).



*Type:*
list of string



*Default:*
` [ ] `



*Example:*

```
[
  "my-namespace"
  "my-other-namespace"
]
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/temporal.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/temporal.nix)



## services.temporal.port



Port for the frontend gRPC service.



*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*
` 7233 `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/temporal.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/temporal.nix)



## services.temporal.state



State configuration.



*Type:*
submodule



*Default:*
` { } `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/temporal.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/temporal.nix)



## services.temporal.state.ephemeral



When enabled, the Temporal state gets lost when the process exists.



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/temporal.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/temporal.nix)



## services.temporal.state.sqlite-pragma



Sqlite pragma statements



*Type:*
attribute set of string



*Default:*
` { } `



*Example:*

```
{
  journal_mode = "wal";
  synchronous = "2";
}
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/temporal.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/temporal.nix)



## services.temporal.ui



UI configuration.



*Type:*
submodule



*Default:*
` { } `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/temporal.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/temporal.nix)



## services.temporal.ui.enable



Enable the Web UI.



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/temporal.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/temporal.nix)



## services.temporal.ui.ip



IPv4 address to bind the Web UI to.



*Type:*
string



*Default:*
` "127.0.0.1" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/temporal.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/temporal.nix)



## services.temporal.ui.port



Port for the Web UI.



*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*
[` services.temporal.port `](\#servicestemporalport) + 1000

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/temporal.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/temporal.nix)



## services.varnish.enable



Whether to enable Varnish process and expose utilities.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/varnish.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/varnish.nix)



## services.varnish.package



Which Varnish package to use.



*Type:*
package



*Default:*
` pkgs.varnish `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/varnish.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/varnish.nix)



## services.varnish.extraModules



Varnish modules (except ‘std’).



*Type:*
list of package



*Default:*
` [ ] `



*Example:*
` [ pkgs.varnish73Packages.modules ] `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/varnish.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/varnish.nix)



## services.varnish.listen



Which address to listen on.



*Type:*
string



*Default:*
` "127.0.0.1:6081" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/varnish.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/varnish.nix)



## services.varnish.memorySize



How much memory to allocate to Varnish.



*Type:*
string



*Default:*
` "64M" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/varnish.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/varnish.nix)



## services.varnish.vcl



Varnish VCL configuration.



*Type:*
strings concatenated with “\\n”



*Default:*

```
''
  vcl 4.0;
  
  backend default {
    .host = "127.0.0.1";
    .port = "80";
  }
''
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/varnish.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/varnish.nix)



## services.vault.enable



Whether to enable vault process.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/vault.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/vault.nix)



## services.vault.package



Which package of Vault to use.



*Type:*
package



*Default:*
` pkgs.vault-bin `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/vault.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/vault.nix)



## services.vault.address



Specifies the address to bind to for listening



*Type:*
string



*Default:*
` "127.0.0.1:8200" `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/vault.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/vault.nix)



## services.vault.disableClustering



Specifies whether clustering features such as request forwarding are enabled



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/vault.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/vault.nix)



## services.vault.disableMlock



Disables the server from executing the mlock syscall



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/vault.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/vault.nix)



## services.vault.ui



Enables the built-in web UI



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/vault.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/vault.nix)



## services.wiremock.enable



Whether to enable WireMock.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/wiremock.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/wiremock.nix)



## services.wiremock.package



Which package of WireMock to use.



*Type:*
package



*Default:*
` pkgs.wiremock `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/wiremock.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/wiremock.nix)



## services.wiremock.disableBanner



Whether to disable print banner logo.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/wiremock.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/wiremock.nix)



## services.wiremock.mappings



The mappings to mock.
See the JSON examples on [https://wiremock.org/docs/stubbing/](https://wiremock.org/docs/stubbing/) for more information.



*Type:*
JSON value



*Default:*
` [ ] `



*Example:*

```
[
  {
    request = {
      method = "GET";
      url = "/body";
    };
    response = {
      body = "Literal text to put in the body";
      headers = {
        Content-Type = "text/plain";
      };
      status = 200;
    };
  }
  {
    request = {
      method = "GET";
      url = "/json";
    };
    response = {
      jsonBody = {
        someField = "someValue";
      };
      status = 200;
    };
  }
]
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/wiremock.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/wiremock.nix)



## services.wiremock.port



The port number for the HTTP server to listen on.



*Type:*
signed integer



*Default:*
` 8080 `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/wiremock.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/wiremock.nix)



## services.wiremock.verbose



Whether to log verbosely to stdout.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/services/wiremock.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/wiremock.nix)



## starship.enable



Whether to enable the Starship.rs command prompt.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/starship.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/starship.nix)



## starship.package



The Starship package to use.



*Type:*
package



*Default:*
` pkgs.starship `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/starship.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/starship.nix)



## starship.config.enable



Whether to enable Starship config override.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/starship.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/starship.nix)



## starship.config.path



The Starship configuration file to use.



*Type:*
path



*Default:*
` ${config.env.DEVENV_ROOT}/starship.toml `

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/integrations/starship.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/starship.nix)



## unsetEnvVars



Remove these list of env vars from being exported to keep the shell/direnv more lean.



*Type:*
list of string



*Default:*

```
[
  "HOST_PATH"
  "NIX_BUILD_CORES"
  "__structuredAttrs"
  "buildInputs"
  "buildPhase"
  "builder"
  "depsBuildBuild"
  "depsBuildBuildPropagated"
  "depsBuildTarget"
  "depsBuildTargetPropagated"
  "depsHostHost"
  "depsHostHostPropagated"
  "depsTargetTarget"
  "depsTargetTargetPropagated"
  "doCheck"
  "doInstallCheck"
  "nativeBuildInputs"
  "out"
  "outputs"
  "patches"
  "phases"
  "preferLocalBuild"
  "propagatedBuildInputs"
  "propagatedNativeBuildInputs"
  "shell"
  "shellHook"
  "stdenv"
  "strictDeps"
]
```

*Declared by:*
 - [https://github.com/cachix/devenv/blob/main/src/modules/top-level.nix](https://github.com/cachix/devenv/blob/main/src/modules/top-level.nix)


