# devenv.nix options

## packages

A list of packages to expose inside the developer environment. Search available packages using ``devenv search NAME``.



*Type:*
list of package



*Default:*
` [ ] `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/top-level\.nix](https://github.com/cachix/devenv/blob/main/src/modules/top-level.nix)



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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/integrations/mkcert\.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/mkcert.nix)



## container\.isBuilding

Set to true when the environment is building a container.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/containers\.nix](https://github.com/cachix/devenv/blob/main/src/modules/containers.nix)



## containers

Container specifications that can be built, copied and ran using `devenv container`.



*Type:*
attribute set of (submodule)



*Default:*
` { } `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/containers\.nix](https://github.com/cachix/devenv/blob/main/src/modules/containers.nix)



## containers\.\<name>\.copyToRoot

Add a path to the container. Defaults to the whole git repo.



*Type:*
null or path



*Default:*
` "self" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/containers\.nix](https://github.com/cachix/devenv/blob/main/src/modules/containers.nix)



## containers\.\<name>\.defaultCopyArgs

Default arguments to pass to `skopeo copy`.
You can override them by passing arguments to the script.




*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/containers\.nix](https://github.com/cachix/devenv/blob/main/src/modules/containers.nix)



## containers\.\<name>\.entrypoint

Entrypoint of the container.



*Type:*
list of anything



*Default:*
` [ entrypoint ] `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/containers\.nix](https://github.com/cachix/devenv/blob/main/src/modules/containers.nix)



## containers\.\<name>\.isBuilding

Set to true when the environment is building this container.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/containers\.nix](https://github.com/cachix/devenv/blob/main/src/modules/containers.nix)



## containers\.\<name>\.name

Name of the container.



*Type:*
null or string



*Default:*
` "top-level name or containers.mycontainer.name" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/containers\.nix](https://github.com/cachix/devenv/blob/main/src/modules/containers.nix)



## containers\.\<name>\.registry

Registry to push the container to.



*Type:*
null or string



*Default:*
` "docker://" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/containers\.nix](https://github.com/cachix/devenv/blob/main/src/modules/containers.nix)



## containers\.\<name>\.startupCommand

Command to run in the container.



*Type:*
null or string or package



*Default:*
` null `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/containers\.nix](https://github.com/cachix/devenv/blob/main/src/modules/containers.nix)



## containers\.\<name>\.version

Version/tag of the container.



*Type:*
null or string



*Default:*
` "latest" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/containers\.nix](https://github.com/cachix/devenv/blob/main/src/modules/containers.nix)



## devcontainer\.enable

Whether to enable generation .devcontainer.json for devenv integration.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/integrations/devcontainer\.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/devcontainer.nix)



## devenv\.flakesIntegration

Tells if devenv is being imported by a flake.nix file




*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/update-check\.nix](https://github.com/cachix/devenv/blob/main/src/modules/update-check.nix)



## devenv\.latestVersion

The latest version of devenv.




*Type:*
string



*Default:*
` "0.6.2" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/update-check\.nix](https://github.com/cachix/devenv/blob/main/src/modules/update-check.nix)



## devenv\.warnOnNewVersion

Whether to warn when a new version of devenv is available.




*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/update-check\.nix](https://github.com/cachix/devenv/blob/main/src/modules/update-check.nix)



## difftastic\.enable

Integrate difftastic into git: https://difftastic.wilfred.me.uk/.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/integrations/difftastic\.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/difftastic.nix)



## enterShell

Bash code to execute when entering the shell.



*Type:*
strings concatenated with “\\n”



*Default:*
` "" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/top-level\.nix](https://github.com/cachix/devenv/blob/main/src/modules/top-level.nix)



## env

Environment variables to be exposed inside the developer environment.



*Type:*
lazy attribute set of anything



*Default:*
` { } `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/top-level\.nix](https://github.com/cachix/devenv/blob/main/src/modules/top-level.nix)



## hosts

List of hosts entries.



*Type:*
attribute set of string



*Default:*
` { } `



*Example:*

```
{
  "example.com" = "127.0.0.1";
}
```

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/integrations/hostctl\.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/hostctl.nix)



## hostsProfileName

Profile name to use.



*Type:*
string



*Default:*
` "devenv-e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/integrations/hostctl\.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/hostctl.nix)



## infoSections

Information about the environment



*Type:*
attribute set of list of string



*Default:*
` { } `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/info\.nix](https://github.com/cachix/devenv/blob/main/src/modules/info.nix)



## languages\.ansible\.enable

Whether to enable tools for Ansible development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/ansible\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/ansible.nix)



## languages\.ansible\.package

The Ansible package to use.



*Type:*
package



*Default:*
` pkgs.ansible `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/ansible\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/ansible.nix)



## languages\.c\.enable

Whether to enable tools for C development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/c\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/c.nix)



## languages\.clojure\.enable

Whether to enable tools for Clojure development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/clojure\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/clojure.nix)



## languages\.cplusplus\.enable

Whether to enable tools for C++ development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/cplusplus\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/cplusplus.nix)



## languages\.crystal\.enable

Whether to enable Enable tools for Crystal development..



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/crystal\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/crystal.nix)



## languages\.cue\.enable

Whether to enable tools for Cue development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/cue\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/cue.nix)



## languages\.cue\.package

The CUE package to use.



*Type:*
package



*Default:*
` pkgs.cue `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/cue\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/cue.nix)



## languages\.dart\.enable

Whether to enable tools for Dart development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/dart\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/dart.nix)



## languages\.dart\.package

The Dart package to use.



*Type:*
package



*Default:*
` pkgs.dart `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/dart\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/dart.nix)



## languages\.deno\.enable

Whether to enable tools for Deno development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/deno\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/deno.nix)



## languages\.dotnet\.enable

Whether to enable tools for .NET development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/dotnet\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/dotnet.nix)



## languages\.elixir\.enable

Whether to enable tools for Elixir development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/elixir\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix)



## languages\.elixir\.package

Which package of Elixir to use.



*Type:*
package



*Default:*
` pkgs.elixir `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/elixir\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/elixir.nix)



## languages\.elm\.enable

Whether to enable tools for Elm development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/elm\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/elm.nix)



## languages\.erlang\.enable

Whether to enable tools for Erlang development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/erlang\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/erlang.nix)



## languages\.erlang\.package

Which package of Erlang to use.



*Type:*
package



*Default:*
` pkgs.erlang `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/erlang\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/erlang.nix)



## languages\.gawk\.enable

Whether to enable tools for GNU Awk development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/gawk\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/gawk.nix)



## languages\.go\.enable

Whether to enable tools for Go development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/go\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/go.nix)



## languages\.go\.package

The Go package to use.



*Type:*
package



*Default:*
` pkgs.go `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/go\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/go.nix)



## languages\.haskell\.enable

Whether to enable tools for Haskell development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/haskell\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/haskell.nix)



## languages\.java\.enable

Whether to enable tools for Java development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/java\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/java.nix)



## languages\.java\.gradle\.enable

Whether to enable gradle.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/java\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/java.nix)



## languages\.java\.gradle\.package

The Gradle package to use.
The Gradle package by default inherits the JDK from `languages.java.jdk.package`.




*Type:*
package

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/java\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/java.nix)



## languages\.java\.jdk\.package

The JDK package to use.
This will also become available as `JAVA_HOME`.




*Type:*
package



*Default:*
` pkgs.jdk `



*Example:*
` pkgs.jdk8 `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/java\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/java.nix)



## languages\.java\.maven\.enable

Whether to enable maven.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/java\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/java.nix)



## languages\.java\.maven\.package

The Maven package to use.
The Maven package by default inherits the JDK from `languages.java.jdk.package`.




*Type:*
package

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/java\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/java.nix)



## languages\.javascript\.enable

Whether to enable tools for JavaScript development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/javascript\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix)



## languages\.javascript\.package

The Node package to use.



*Type:*
package



*Default:*
` pkgs.nodejs `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/javascript\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/javascript.nix)



## languages\.julia\.enable

Whether to enable tools for Julia development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/julia\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/julia.nix)



## languages\.julia\.package

The Julia package to use.



*Type:*
package



*Default:*
` pkgs.julia-bin `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/julia\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/julia.nix)



## languages\.kotlin\.enable

Whether to enable tools for Kotlin development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/kotlin\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/kotlin.nix)



## languages\.lua\.enable

Whether to enable tools for Lua development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/lua\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/lua.nix)



## languages\.lua\.package

The Lua package to use.



*Type:*
package



*Default:*
` pkgs.lua `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/lua\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/lua.nix)



## languages\.nim\.enable

Whether to enable tools for Nim development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/nim\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/nim.nix)



## languages\.nim\.package

The Nim package to use.



*Type:*
package



*Default:*
` pkgs.nim `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/nim\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/nim.nix)



## languages\.nix\.enable

Whether to enable tools for Nix development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/nix\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/nix.nix)



## languages\.ocaml\.enable

Whether to enable tools for OCaml development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/ocaml\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/ocaml.nix)



## languages\.ocaml\.packages

The package set of OCaml to use



*Type:*
attribute set



*Default:*
` pkgs.ocaml-ng.ocamlPackages_4_12 `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/ocaml\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/ocaml.nix)



## languages\.perl\.enable

Whether to enable tools for Perl development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/perl\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/perl.nix)



## languages\.php\.enable

Whether to enable tools for PHP development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/php\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages\.php\.package

Allows you to [override the default used package](https://nixos.org/manual/nixpkgs/stable/#ssec-php-user-guide)
to adjust the settings or add more extensions. You can find the
extensions using `devenv search 'php extensions'`




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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/php\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages\.php\.packages

Attribute set of packages including composer



*Type:*
submodule



*Default:*
` pkgs `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/php\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages\.php\.packages\.composer

composer package



*Type:*
null or package



*Default:*
` pkgs.phpPackages.composer `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/php\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages\.php\.extensions

PHP extensions to enable.




*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/php\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages\.php\.fpm\.extraConfig

Extra configuration that should be put in the global section of
the PHP-FPM configuration file. Do not specify the options
`error_log` or `daemonize` here, since they are generated by
NixOS.




*Type:*
null or strings concatenated with “\\n”



*Default:*
` null `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/php\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages\.php\.fpm\.phpOptions

Options appended to the PHP configuration file `php.ini`.




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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/php\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages\.php\.fpm\.pools

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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/php\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages\.php\.fpm\.pools\.\<name>\.extraConfig

Extra lines that go into the pool configuration.
See the documentation on `php-fpm.conf` for
details on configuration directives.




*Type:*
null or strings concatenated with “\\n”



*Default:*
` null `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/php\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages\.php\.fpm\.pools\.\<name>\.listen

The address on which to accept FastCGI requests.




*Type:*
string



*Default:*
` "" `



*Example:*
` "/path/to/unix/socket" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/php\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages\.php\.fpm\.pools\.\<name>\.phpEnv

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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/php\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages\.php\.fpm\.pools\.\<name>\.phpOptions

Options appended to the PHP configuration file `php.ini` used for this PHP-FPM pool.




*Type:*
strings concatenated with “\\n”

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/php\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages\.php\.fpm\.pools\.\<name>\.phpPackage

The PHP package to use for running this PHP-FPM pool.




*Type:*
package



*Default:*
` phpfpm.phpPackage `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/php\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages\.php\.fpm\.pools\.\<name>\.settings

PHP-FPM pool directives. Refer to the "List of pool directives" section of
<https://www.php.net/manual/en/install.fpm.configuration.php">
the manual for details. Note that settings names must be
enclosed in quotes (e.g. `"pm.max_children"` instead of
`pm.max_children`).




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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/php\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages\.php\.fpm\.pools\.\<name>\.socket

Path to the Unix socket file on which to accept FastCGI requests.

This option is read-only and managed by NixOS.




*Type:*
string *(read only)*



*Example:*
` "/.devenv/state/php-fpm/<name>.sock" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/php\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages\.php\.fpm\.settings

PHP-FPM global directives. 

Refer to the "List of global php-fpm.conf directives" section of
<https://www.php.net/manual/en/install.fpm.configuration.php>
for details. 

Note that settings names must be enclosed in
quotes (e.g. `"pm.max_children"` instead of `pm.max_children`). 

You need not specify the options `error_log` or `daemonize` here, since
they are already set.




*Type:*
attribute set of (string or signed integer or boolean)



*Default:*

```
{
  error_log = "/.devenv/state/php-fpm/php-fpm.log";
}
```

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/php\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages\.php\.ini

PHP.ini directives. Refer to the "List of php.ini directives" of PHP's




*Type:*
null or strings concatenated with “\\n”



*Default:*
` "" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/php\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages\.php\.version

The PHP version to use.



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/php\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/php.nix)



## languages\.purescript\.enable

Whether to enable tools for PureScript development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/purescript\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/purescript.nix)



## languages\.purescript\.package

The PureScript package to use.



*Type:*
package



*Default:*
` pkgs.purescript `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/purescript\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/purescript.nix)



## languages\.python\.enable

Whether to enable tools for Python development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/python\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix)



## languages\.python\.package

The Python package to use.



*Type:*
package



*Default:*
` pkgs.python3 `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/python\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix)



## languages\.python\.poetry\.enable

Whether to enable poetry.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/python\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix)



## languages\.python\.poetry\.package

The Poetry package to use.



*Type:*
package



*Default:*

```
pkgs.poetry.override {
  python3 = config.languages.python.package;
}

```

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/python\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix)



## languages\.python\.venv\.enable

Whether to enable Python virtual environment.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/python\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/python.nix)



## languages\.r\.enable

Whether to enable tools for R development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/r\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/r.nix)



## languages\.r\.package

The R package to use.



*Type:*
package



*Default:*
` pkgs.R `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/r\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/r.nix)



## languages\.racket\.enable

Whether to enable tools for Racket development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/racket\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/racket.nix)



## languages\.racket\.package

The Racket package to use.



*Type:*
package



*Default:*
` pkgs.racket-minimal `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/racket\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/racket.nix)



## languages\.raku\.enable

Whether to enable tools for Raku development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/raku\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/raku.nix)



## languages\.robotframework\.enable

Whether to enable tools for Robot Framework development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/robotframework\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/robotframework.nix)



## languages\.robotframework\.python

The Python package to use.



*Type:*
package



*Default:*
` pkgs.python3 `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/robotframework\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/robotframework.nix)



## languages\.ruby\.enable

Whether to enable tools for Ruby development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/ruby\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/ruby.nix)



## languages\.ruby\.package

The Ruby package to use.



*Type:*
package



*Default:*
` pkgs.ruby_3_1 `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/ruby\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/ruby.nix)



## languages\.ruby\.version

The Ruby version to use.
This automatically sets the `languages.ruby.package` using [nixpkgs-ruby](https://github.com/bobvanderlinden/nixpkgs-ruby).




*Type:*
null or string



*Default:*
` null `



*Example:*
` "3.2.1" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/ruby\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/ruby.nix)



## languages\.ruby\.versionFile

The .ruby-version file path to extract the Ruby version from.
This automatically sets the `languages.ruby.package` using [nixpkgs-ruby](https://github.com/bobvanderlinden/nixpkgs-ruby).
When the `.ruby-version` file exists in the same directory as the devenv configuration, you can use:

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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/ruby\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/ruby.nix)



## languages\.rust\.enable

Whether to enable tools for Rust development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/rust\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



## languages\.rust\.packages

Attribute set of packages including rustc and Cargo.



*Type:*
submodule



*Default:*
` pkgs `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/rust\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



## languages\.rust\.packages\.cargo

cargo package



*Type:*
package



*Default:*
` pkgs.cargo `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/rust\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



## languages\.rust\.packages\.clippy

clippy package

*Type:*
package



*Default:*
` pkgs.clippy `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/rust\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



## languages\.rust\.packages\.rust-analyzer

rust-analyzer package



*Type:*
package



*Default:*
` pkgs.rust-analyzer `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/rust\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



## languages\.rust\.packages\.rust-src

rust-src package



*Type:*
package or string



*Default:*
` pkgs.rustPlatform.rustLibSrc `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/rust\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



## languages\.rust\.packages\.rustc

rustc package



*Type:*
package



*Default:*
` pkgs.rustc `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/rust\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



## languages\.rust\.packages\.rustfmt

rustfmt package



*Type:*
package



*Default:*
` pkgs.rustfmt `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/rust\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



## languages\.rust\.version

Set to stable, beta, or latest.



*Type:*
null or string



*Default:*
` null `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/rust\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/rust.nix)



## languages\.scala\.enable

Whether to enable tools for Scala development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/scala\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/scala.nix)



## languages\.scala\.package

The Scala package to use.




*Type:*
package



*Default:*
` "pkgs.scala_3" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/scala\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/scala.nix)



## languages\.swift\.enable

Whether to enable tools for Swift development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/swift\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/swift.nix)



## languages\.swift\.package

The Swift package to use.




*Type:*
package



*Default:*
` "pkgs.swift" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/swift\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/swift.nix)



## languages\.terraform\.enable

Whether to enable tools for Terraform development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/terraform\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/terraform.nix)



## languages\.terraform\.package

The Terraform package to use.



*Type:*
package



*Default:*
` pkgs.terraform `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/terraform\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/terraform.nix)



## languages\.texlive\.enable

Whether to enable TeX Live.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/texlive\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/texlive.nix)



## languages\.texlive\.packages

Packages available to TeX Live



*Type:*
non-empty (list of Concatenated string)



*Default:*

```
[
  "collection-basic"
]
```

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/texlive\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/texlive.nix)



## languages\.texlive\.base

TeX Live package set to use



*Type:*
unspecified value



*Default:*
` pkgs.texlive `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/texlive\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/texlive.nix)



## languages\.typescript\.enable

Whether to enable tools for TypeScript development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/typescript\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/typescript.nix)



## languages\.unison\.enable

Whether to enable tools for Unison development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/unison\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/unison.nix)



## languages\.unison\.package

Which package of Unison to use



*Type:*
package



*Default:*
` pkgs.unison-ucm `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/unison\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/unison.nix)



## languages\.v\.enable

Whether to enable tools for V development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/v\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/v.nix)



## languages\.v\.package

The V package to use.



*Type:*
package



*Default:*
` pkgs.vlang `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/v\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/v.nix)



## languages\.zig\.enable

Whether to enable tools for Zig development.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/zig\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/zig.nix)



## languages\.zig\.package

Which package of Zig to use.



*Type:*
package



*Default:*
` pkgs.zig `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/languages/zig\.nix](https://github.com/cachix/devenv/blob/main/src/modules/languages/zig.nix)



## name

Name of the project.



*Type:*
null or string



*Default:*
` null `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/top-level\.nix](https://github.com/cachix/devenv/blob/main/src/modules/top-level.nix)



## pre-commit

Integration of https://github.com/cachix/pre-commit-hooks.nix



*Type:*
submodule



*Default:*
` { } `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/integrations/pre-commit\.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/pre-commit.nix)



## pre-commit\.package



The ` pre-commit ` package to use\.



*Type:*
package

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/pre-commit\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit\.default_stages



A configuration wide option for the stages property\.
Installs hooks to the defined stages\.
See [https://pre-commit\.com/\#confining-hooks-to-run-at-certain-stages](https://pre-commit\.com/\#confining-hooks-to-run-at-certain-stages)\.



*Type:*
list of string



*Default:*

```
[
  "commit"
]
```

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/pre-commit\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit\.excludes



Exclude files that were matched by these patterns\.



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/pre-commit\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit\.hooks



The hook definitions\.

Pre-defined hooks can be enabled by, for example:

```nix
hooks.nixpkgs-fmt.enable = true;
```

The pre-defined hooks are:

**` actionlint `**

Static checker for GitHub Actions workflow files\.

**` alejandra `**

The Uncompromising Nix Code Formatter\.

**` ansible-lint `**

Ansible linter\.

**` autoflake `**

Remove unused imports and variables from Python code\.

**` bats `**

Run bash unit tests\.

**` black `**

The uncompromising Python code formatter\.

**` cabal-fmt `**

Format Cabal files

**` cabal2nix `**

Run ` cabal2nix ` on all ` *.cabal ` files to generate corresponding ` default.nix ` files\.

**` cargo-check `**

Check the cargo package for errors\.

**` chktex `**

LaTeX semantic checker

**` clang-format `**

Format your code using ` clang-format `\.

**` clippy `**

Lint Rust code\.

**` commitizen `**

Check whether the current commit message follows commiting rules\.

**` deadnix `**

Scan Nix files for dead code (unused variable bindings)\.

**` dhall-format `**

Dhall code formatter\.

**` dune-opam-sync `**

Check that Dune-generated OPAM files are in sync\.

**` editorconfig-checker `**

Verify that the files are in harmony with the ` .editorconfig `\.

**` elm-format `**

Format Elm files\.

**` elm-review `**

Analyzes Elm projects, to help find mistakes before your users find them\.

**` elm-test `**

Run unit tests and fuzz tests for Elm code\.

**` eslint `**

Find and fix problems in your JavaScript code\.

**` flake8 `**

Check the style and quality of Python files\.

**` fourmolu `**

Haskell code prettifier\.

**` gofmt `**

A tool that automatically formats Go source code

**` gotest `**

Run go tests

**` govet `**

Checks correctness of Go programs\.

**` hadolint `**

Dockerfile linter, validate inline bash\.

**` hindent `**

Haskell code prettifier\.

**` hlint `**

HLint gives suggestions on how to improve your source code\.

**` hpack `**

` hpack ` converts package definitions in the hpack format (` package.yaml `) to Cabal files\.

**` html-tidy `**

HTML linter\.

**` hunspell `**

Spell checker and morphological analyzer\.

**` isort `**

A Python utility / library to sort imports\.

**` latexindent `**

Perl script to add indentation to LaTeX files\.

**` luacheck `**

A tool for linting and static analysis of Lua code\.

**` markdownlint `**

Style checker and linter for markdown files\.

**` mdsh `**

Markdown shell pre-processor\.

**` nixfmt `**

Nix code prettifier\.

**` nixpkgs-fmt `**

Nix code prettifier\.

**` ocp-indent `**

A tool to indent OCaml code\.

**` opam-lint `**

OCaml package manager configuration checker\.

**` ormolu `**

Haskell code prettifier\.

**` php-cs-fixer `**

Lint PHP files\.

**` phpcbf `**

Lint PHP files\.

**` phpcs `**

Lint PHP files\.

**` prettier `**

Opinionated multi-language code formatter\.

**` purs-tidy `**

Format purescript files\.

**` purty `**

Format purescript files\.

**` pylint `**

Lint Python files\.

**` revive `**

A linter for Go source code\.

**` ruff `**

An extremely fast Python linter, written in Rust\.

**` rustfmt `**

Format Rust code\.

**` shellcheck `**

Format shell files\.

**` shfmt `**

Format shell files\.

**` staticcheck `**

State of the art linter for the Go programming language

**` statix `**

Lints and suggestions for the Nix programming language\.

**` stylish-haskell `**

A simple Haskell code prettifier

**` stylua `**

An Opinionated Lua Code Formatter\.

**` taplo `**

Format TOML files with taplo fmt

**` terraform-format `**

Format terraform (` .tf `) files\.

**` typos `**

Source code spell checker

**` yamllint `**

Yaml linter\.

**` zprint `**

Beautifully format Clojure and Clojurescript source code and s-expressions\.



*Type:*
attribute set of (submodule)



*Default:*
` { } `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/pre-commit\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit\.hooks\.\<name>\.enable



Whether to enable this pre-commit hook\.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/pre-commit\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit\.hooks\.\<name>\.description



Description of the hook\. used for metadata purposes only\.



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/pre-commit\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit\.hooks\.\<name>\.entry



The entry point - the executable to run\. ` entry ` can also contain arguments that will not be overridden, such as ` entry = "autopep8 -i"; `\.



*Type:*
string

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/pre-commit\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit\.hooks\.\<name>\.excludes



Exclude files that were matched by these patterns\.



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/pre-commit\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit\.hooks\.\<name>\.fail_fast



if true pre-commit will stop running hooks if this hook fails\.



*Type:*
boolean

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/pre-commit\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit\.hooks\.\<name>\.files



The pattern of files to run on\.



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/pre-commit\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit\.hooks\.\<name>\.language



The language of the hook - tells pre-commit how to install the hook\.



*Type:*
string



*Default:*
` "system" `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/pre-commit\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit\.hooks\.\<name>\.name



The name of the hook - shown during hook execution\.



*Type:*
string

*Default:* internal name, same as id

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/pre-commit\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit\.hooks\.\<name>\.pass_filenames



Whether to pass filenames as arguments to the entry point\.



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/pre-commit\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit\.hooks\.\<name>\.raw



Raw fields of a pre-commit hook\. This is mostly for internal use but
exposed in case you need to work around something\.

Default: taken from the other hook options\.



*Type:*
attribute set of unspecified value

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/pre-commit\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit\.hooks\.\<name>\.require_serial



if true this hook will execute using a single process instead of in parallel\.



*Type:*
boolean

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/pre-commit\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit\.hooks\.\<name>\.stages



Confines the hook to run at a particular stage\.



*Type:*
list of string



*Default:*
` default_stages `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/pre-commit\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit\.hooks\.\<name>\.types



List of file types to run on\. See [Filtering files with types](https://pre-commit\.com/\#plugins)\.



*Type:*
list of string



*Default:*

```
[
  "file"
]
```

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/pre-commit\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit\.hooks\.\<name>\.types_or



List of file types to run on, where only a single type needs to match\.



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/pre-commit\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit\.installationScript



A bash snippet that installs nix-pre-commit-hooks in the current directory



*Type:*
string *(read only)*

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/pre-commit\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit\.rootSrc



The source of the project to be checked\.

This is used in the derivation that performs the check\.

If you use the ` flakeModule `, the default is ` self.outPath `; the whole flake
sources\.



*Type:*
path

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/pre-commit\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit\.run



A derivation that tests whether the pre-commit hooks run cleanly on
the entire project\.



*Type:*
package *(read only)*



*Default:*
` "<derivation>" `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/pre-commit\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit\.settings\.alejandra\.exclude



Files or directories to exclude from formatting\.



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
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.autoflake\.binPath



Path to autoflake binary\.



*Type:*
string



*Default:*

```
"${pkgs.autoflake}/bin/autoflake"

```

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.autoflake\.flags



Flags passed to autoflake\.



*Type:*
string



*Default:*
` "--in-place --expand-star-imports --remove-duplicate-keys --remove-unused-variables" `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.clippy\.denyWarnings



Fail when warnings are present



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.clippy\.offline



Run clippy offline



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.deadnix\.edit



Remove unused code and write to source file\.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.deadnix\.noLambdaArg



Don’t check lambda parameter arguments\.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.deadnix\.noLambdaPatternNames



Don’t check lambda pattern names (don’t break nixpkgs ` callPackage `)\.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.deadnix\.noUnderscore



Don’t check any bindings that start with a ` _ `\.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.deadnix\.quiet



Don’t print a dead code report\.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.eslint\.binPath



` eslint ` binary path\. E\.g\. if you want to use the ` eslint ` in ` node_modules `, use ` ./node_modules/.bin/eslint `\.



*Type:*
path



*Default:*
` ${tools.eslint}/bin/eslint `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.eslint\.extensions



The pattern of files to run on, see [https://pre-commit\.com/\#hooks-files](https://pre-commit\.com/\#hooks-files)\.



*Type:*
string



*Default:*
` "\\.js$" `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.flake8\.binPath



flake8 binary path\. Should be used to specify flake8 binary from your Nix-managed Python environment\.



*Type:*
string



*Default:*

```
"${pkgs.python39Packages.flake8}/bin/flake8"

```

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.flake8\.format



Output format\.



*Type:*
string



*Default:*
` "default" `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.hpack\.silent



Whether generation should be silent\.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.markdownlint\.config



See https://github\.com/DavidAnson/markdownlint/blob/main/schema/\.markdownlint\.jsonc



*Type:*
attribute set



*Default:*
` { } `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.nixfmt\.width



Line width\.



*Type:*
null or signed integer



*Default:*
` null `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.ormolu\.cabalDefaultExtensions



Use ` default-extensions ` from ` .cabal ` files\.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.ormolu\.defaultExtensions



Haskell language extensions to enable\.



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.php-cs-fixer\.binPath



PHP-CS-Fixer binary path\.



*Type:*
string



*Default:*

```
"${pkgs.php81Packages.php-cs-fixer}/bin/php-cs-fixer"

```

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.phpcbf\.binPath



PHP_CodeSniffer binary path\.



*Type:*
string



*Default:*

```
"${pkgs.php80Packages.phpcbf}/bin/phpcbf"

```

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.phpcs\.binPath



PHP_CodeSniffer binary path\.



*Type:*
string



*Default:*

```
"${pkgs.php80Packages.phpcs}/bin/phpcs"

```

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.prettier\.binPath



` prettier ` binary path\. E\.g\. if you want to use the ` prettier ` in ` node_modules `, use ` ./node_modules/.bin/prettier `\.



*Type:*
path



*Default:*

```
"${tools.prettier}/bin/prettier"

```

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.prettier\.output



Output format\.



*Type:*
null or one of “check”, “list-different”



*Default:*
` "list-different" `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.prettier\.write



Whether to edit files inplace\.



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.pylint\.binPath



Pylint binary path\. Should be used to specify Pylint binary from your Nix-managed Python environment\.



*Type:*
string



*Default:*

```
"${pkgs.python39Packages.pylint}/bin/pylint"

```

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.pylint\.reports



Whether to display a full report\.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.pylint\.score



Whether to activate the evaluation score\.



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.revive\.configPath



Path to the configuration TOML file\.



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.rust\.cargoManifestPath



Path to Cargo\.toml



*Type:*
null or string



*Default:*
` null `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.statix\.format



Error Output format\.



*Type:*
one of “stderr”, “errfmt”, “json”



*Default:*
` "errfmt" `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.statix\.ignore



Globs of file patterns to skip\.



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
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.typos\.diff



Wheter to print a diff of what would change\.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.typos\.format



Output format\.



*Type:*
one of “silent”, “brief”, “long”, “json”



*Default:*
` "long" `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.typos\.write



Whether to write fixes out\.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.yamllint\.configPath

path to the configuration YAML file



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.settings\.yamllint\.relaxed



Use the relaxed configuration



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/hooks\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/hooks.nix)



## pre-commit\.src



Root of the project\. By default this will be filtered with the ` gitignoreSource `
function later, unless ` rootSrc ` is specified\.

If you use the ` flakeModule `, the default is ` self.outPath `; the whole flake
sources\.



*Type:*
path

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/pre-commit\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## pre-commit\.tools



Tool set from which ` nix-pre-commit-hooks ` will pick binaries\.

` nix-pre-commit-hooks ` comes with its own set of packages for this purpose\.



*Type:*
lazy attribute set of package

*Declared by:*
 - [https://github\.com/cachix/pre-commit-hooks\.nix/blob/master/modules/pre-commit\.nix](https://github.com/cachix/pre-commit-hooks.nix/blob/master/modules/pre-commit.nix)



## process\.after

Bash code to execute after stopping processes.



*Type:*
strings concatenated with “\\n”



*Default:*
` "" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/processes\.nix](https://github.com/cachix/devenv/blob/main/src/modules/processes.nix)



## process\.before

Bash code to execute before starting processes.



*Type:*
strings concatenated with “\\n”



*Default:*
` "" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/processes\.nix](https://github.com/cachix/devenv/blob/main/src/modules/processes.nix)



## process\.implementation

The implementation used when performing ``devenv up``.



*Type:*
one of “honcho”, “overmind”, “process-compose”, “hivemind”



*Default:*
` "honcho" `



*Example:*
` "overmind" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/processes\.nix](https://github.com/cachix/devenv/blob/main/src/modules/processes.nix)



## process\.process-compose

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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/processes\.nix](https://github.com/cachix/devenv/blob/main/src/modules/processes.nix)



## processes

Processes can be started with ``devenv up`` and run in foreground mode.



*Type:*
attribute set of (submodule)



*Default:*
` { } `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/processes\.nix](https://github.com/cachix/devenv/blob/main/src/modules/processes.nix)



## processes\.\<name>\.exec

Bash code to run the process.



*Type:*
string

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/processes\.nix](https://github.com/cachix/devenv/blob/main/src/modules/processes.nix)



## processes\.\<name>\.process-compose

process-compose.yaml specific process attributes.

Example: https://github.com/F1bonacc1/process-compose/blob/main/process-compose.yaml`

Only used when using ``process.implementation = "process-compose";``




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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/processes\.nix](https://github.com/cachix/devenv/blob/main/src/modules/processes.nix)



## scripts

A set of scripts available when the environment is active.



*Type:*
attribute set of (submodule)



*Default:*
` { } `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/scripts\.nix](https://github.com/cachix/devenv/blob/main/src/modules/scripts.nix)



## scripts\.\<name>\.exec

Bash code to execute when the script is run.



*Type:*
string

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/scripts\.nix](https://github.com/cachix/devenv/blob/main/src/modules/scripts.nix)



## services\.adminer\.enable

Whether to enable Adminer process.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/adminer\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/adminer.nix)



## services\.adminer\.package

Which package of Adminer to use.



*Type:*
package



*Default:*
` pkgs.adminer `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/adminer\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/adminer.nix)



## services\.adminer\.listen

Listen address for the Adminer.



*Type:*
string



*Default:*
` "127.0.0.1:8080" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/adminer\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/adminer.nix)



## services\.blackfire\.enable

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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/blackfire\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/blackfire.nix)



## services\.blackfire\.package

Which package of blackfire to use



*Type:*
package



*Default:*
` pkgs.blackfire `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/blackfire\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/blackfire.nix)



## services\.blackfire\.client-id

Sets the client id used to authenticate with Blackfire.
You can find your personal client-id at <https://blackfire.io/my/settings/credentials>.




*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/blackfire\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/blackfire.nix)



## services\.blackfire\.client-token

Sets the client token used to authenticate with Blackfire.
You can find your personal client-token at <https://blackfire.io/my/settings/credentials>.




*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/blackfire\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/blackfire.nix)



## services\.blackfire\.server-id

Sets the server id used to authenticate with Blackfire.
You can find your personal server-id at <https://blackfire.io/my/settings/credentials>.




*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/blackfire\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/blackfire.nix)



## services\.blackfire\.server-token

Sets the server token used to authenticate with Blackfire.
You can find your personal server-token at <https://blackfire.io/my/settings/credentials>.




*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/blackfire\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/blackfire.nix)



## services\.blackfire\.socket

Sets the server socket path




*Type:*
string



*Default:*
` "tcp://127.0.0.1:8307" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/blackfire\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/blackfire.nix)



## services\.caddy\.enable

Whether to enable Caddy web server.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/caddy\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix)



## services\.caddy\.package

Caddy package to use.




*Type:*
package



*Default:*
` pkgs.caddy `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/caddy\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix)



## services\.caddy\.adapter

Name of the config adapter to use.
See <https://caddyserver.com/docs/config-adapters> for the full list.




*Type:*
string



*Default:*
` "caddyfile" `



*Example:*
` "nginx" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/caddy\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix)



## services\.caddy\.ca

Certificate authority ACME server. The default (Let's Encrypt
production server) should be fine for most people. Set it to null if
you don't want to include any authority (or if you want to write a more
fine-graned configuration manually).




*Type:*
null or string



*Default:*
` "https://acme-v02.api.letsencrypt.org/directory" `



*Example:*
` "https://acme-staging-v02.api.letsencrypt.org/directory" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/caddy\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix)



## services\.caddy\.config

Verbatim Caddyfile to use.
Caddy v2 supports multiple config formats via adapters (see [`services.caddy.adapter`](#servicescaddyconfig)).




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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/caddy\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix)



## services\.caddy\.dataDir

The data directory, for storing certificates. Before 17.09, this
would create a .caddy directory. With 17.09 the contents of the
.caddy directory are in the specified data directory instead.
Caddy v2 replaced CADDYPATH with XDG directories.
See <https://caddyserver.com/docs/conventions#file-locations>.




*Type:*
path



*Default:*
` "/.devenv/state/caddy" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/caddy\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix)



## services\.caddy\.email

Email address (for Let's Encrypt certificate).



*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/caddy\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix)



## services\.caddy\.resume

Use saved config, if any (and prefer over configuration passed with [`caddy.config`](#caddyconfig)).




*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/caddy\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix)



## services\.caddy\.virtualHosts

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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/caddy\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix)



## services\.caddy\.virtualHosts\.\<name>\.extraConfig

These lines go into the vhost verbatim.




*Type:*
strings concatenated with “\\n”



*Default:*
` "" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/caddy\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix)



## services\.caddy\.virtualHosts\.\<name>\.serverAliases

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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/caddy\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/caddy.nix)



## services\.cassandra\.enable

Whether to enable Add Cassandra process script..



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/cassandra\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/cassandra.nix)



## services\.cassandra\.package

Which version of Cassandra to use



*Type:*
package



*Default:*
` pkgs.cassandra_4 `



*Example:*
` pkgs.cassandra_4; `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/cassandra\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/cassandra.nix)



## services\.cassandra\.allowClients

Enables or disables the native transport server (CQL binary protocol)




*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/cassandra\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/cassandra.nix)



## services\.cassandra\.clusterName

The name of the cluster



*Type:*
string



*Default:*
` "Test Cluster" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/cassandra\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/cassandra.nix)



## services\.cassandra\.extraConfig

Extra options to be merged into `cassandra.yaml` as nix attribute set.




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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/cassandra\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/cassandra.nix)



## services\.cassandra\.jvmOpts

Options to pass to the JVM through the JVM_OPTS environment variable



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/cassandra\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/cassandra.nix)



## services\.cassandra\.listenAddress

Listen address



*Type:*
string



*Default:*
` "127.0.0.1" `



*Example:*
` "127.0.0.1" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/cassandra\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/cassandra.nix)



## services\.cassandra\.seedAddresses

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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/cassandra\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/cassandra.nix)



## services\.couchdb\.enable

Whether to enable CouchDB process.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/couchdb\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/couchdb.nix)



## services\.couchdb\.package

Which version of CouchDB to use



*Type:*
package



*Default:*
` pkgs.couchdb3 `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/couchdb\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/couchdb.nix)



## services\.couchdb\.settings

CouchDB configuration.
to know more about all settings, look at:
<link
  xlink:href="https://docs.couchdb.org/en/stable/config/couchdb.html"
/>




*Type:*
attribute set of attribute set of (INI atom (null, bool, int, float or string))



*Default:*
` { } `



*Example:*

```
{
  couchdb = {
    database_dir = baseDir;
    single_node = true;
    viewIndexDir = baseDir;
    uriFile = "/.devenv/state/couchdb/couchdb.uri";
  };
  admins = {
    "admin_username" = "pass";
  };
  chttpd = {
    bindAddress = "127.0.0.1";
    port = 5984;
    logFile = "/.devenv/state/couchdb/couchdb.log";
  };
}

```

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/couchdb\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/couchdb.nix)



## services\.couchdb\.settings\.chttpd\.bindAddress



Defines the IP address by which CouchDB will be accessible\.



*Type:*
string



*Default:*
` "127.0.0.1" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/couchdb\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/couchdb.nix)



## services\.couchdb\.settings\.chttpd\.logFile



Specifies the location of file for logging output\.



*Type:*
path



*Default:*
` "/.devenv/state/couchdb/couchdb.log" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/couchdb\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/couchdb.nix)



## services\.couchdb\.settings\.chttpd\.port



Defined the port number to listen\.



*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*
` 5984 `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/couchdb\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/couchdb.nix)



## services\.couchdb\.settings\.couchdb\.database_dir

Specifies location of CouchDB database files (*.couch named). This
location should be writable and readable for the user the CouchDB
service runs as (couchdb by default).




*Type:*
path



*Default:*
` "/.devenv/state/couchdb" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/couchdb\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/couchdb.nix)



## services\.couchdb\.settings\.couchdb\.single_node

When this configuration setting is set to true, automatically create
the system databases on startup. Must be set false for a clustered
CouchDB installation.




*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/couchdb\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/couchdb.nix)



## services\.couchdb\.settings\.couchdb\.uriFile

This file contains the full URI that can be used to access this
instance of CouchDB. It is used to help discover the port CouchDB is
running on (if it was set to 0 (e.g. automatically assigned any free
one). This file should be writable and readable for the user that
runs the CouchDB service (couchdb by default).




*Type:*
path



*Default:*
` "/.devenv/state/couchdb/couchdb.uri" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/couchdb\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/couchdb.nix)



## services\.couchdb\.settings\.couchdb\.viewIndexDir

Specifies location of CouchDB view index files. This location should
be writable and readable for the user that runs the CouchDB service
(couchdb by default).




*Type:*
path



*Default:*
` "/.devenv/state/couchdb" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/couchdb\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/couchdb.nix)



## services\.elasticsearch\.enable

Whether to enable elasticsearch.



*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/elasticsearch\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix)



## services\.elasticsearch\.package

Elasticsearch package to use.



*Type:*
package



*Default:*
` pkgs.elasticsearch7 `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/elasticsearch\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix)



## services\.elasticsearch\.cluster_name

Elasticsearch name that identifies your cluster for auto-discovery.



*Type:*
string



*Default:*
` "elasticsearch" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/elasticsearch\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix)



## services\.elasticsearch\.extraCmdLineOptions

Extra command line options for the elasticsearch launcher.



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/elasticsearch\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix)



## services\.elasticsearch\.extraConf

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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/elasticsearch\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix)



## services\.elasticsearch\.extraJavaOptions

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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/elasticsearch\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix)



## services\.elasticsearch\.listenAddress

Elasticsearch listen address.



*Type:*
string



*Default:*
` "127.0.0.1" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/elasticsearch\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix)



## services\.elasticsearch\.logging

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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/elasticsearch\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix)



## services\.elasticsearch\.plugins

Extra elasticsearch plugins



*Type:*
list of package



*Default:*
` [ ] `



*Example:*
` [ pkgs.elasticsearchPlugins.discovery-ec2 ] `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/elasticsearch\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix)



## services\.elasticsearch\.port

Elasticsearch port to listen for HTTP traffic.



*Type:*
signed integer



*Default:*
` 9200 `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/elasticsearch\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix)



## services\.elasticsearch\.single_node

Start a single-node cluster



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/elasticsearch\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix)



## services\.elasticsearch\.tcp_port

Elasticsearch port for the node to node communication.



*Type:*
signed integer



*Default:*
` 9300 `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/elasticsearch\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/elasticsearch.nix)



## services\.mailhog\.enable

Whether to enable mailhog process.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/mailhog\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mailhog.nix)



## services\.mailhog\.package

Which package of mailhog to use



*Type:*
package



*Default:*
` pkgs.mailhog `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/mailhog\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mailhog.nix)



## services\.mailhog\.additionalArgs

Additional arguments passed to `mailhog`.




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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/mailhog\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mailhog.nix)



## services\.mailhog\.apiListenAddress

Listen address for API.



*Type:*
string



*Default:*
` "127.0.0.1:8025" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/mailhog\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mailhog.nix)



## services\.mailhog\.smtpListenAddress

Listen address for SMTP.



*Type:*
string



*Default:*
` "127.0.0.1:1025" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/mailhog\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mailhog.nix)



## services\.mailhog\.uiListenAddress

Listen address for UI.



*Type:*
string



*Default:*
` "127.0.0.1:8025" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/mailhog\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mailhog.nix)



## services\.memcached\.enable

Whether to enable memcached process.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/memcached\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/memcached.nix)



## services\.memcached\.package

Which package of memcached to use



*Type:*
package



*Default:*
` pkgs.memcached `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/memcached\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/memcached.nix)



## services\.memcached\.bind

The IP interface to bind to.
`null` means "all interfaces".




*Type:*
null or string



*Default:*
` "127.0.0.1" `



*Example:*
` "127.0.0.1" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/memcached\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/memcached.nix)



## services\.memcached\.port

The TCP port to accept connections.
If port 0 is specified Redis will not listen on a TCP socket.




*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*
` 11211 `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/memcached\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/memcached.nix)



## services\.memcached\.startArgs

Additional arguments passed to `memcached` during startup.




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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/memcached\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/memcached.nix)



## services\.minio\.enable

Whether to enable MinIO Object Storage.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/minio\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix)



## services\.minio\.package

MinIO package to use.



*Type:*
package



*Default:*
` pkgs.minio `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/minio\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix)



## services\.minio\.accessKey

Access key of 5 to 20 characters in length that clients use to access the server.
This overrides the access key that is generated by MinIO on first startup and stored inside the
`configDir` directory.




*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/minio\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix)



## services\.minio\.browser

Enable or disable access to web UI.



*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/minio\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix)



## services\.minio\.buckets

List of buckets to ensure exist on startup.




*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/minio\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix)



## services\.minio\.consoleAddress

IP address and port of the web UI (console).



*Type:*
string



*Default:*
` "127.0.0.1:9001" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/minio\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix)



## services\.minio\.listenAddress

IP address and port of the server.



*Type:*
string



*Default:*
` "127.0.0.1:9000" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/minio\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix)



## services\.minio\.region

The physical location of the server. By default it is set to us-east-1, which is same as AWS S3's and MinIO's default region.




*Type:*
string



*Default:*
` "us-east-1" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/minio\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix)



## services\.minio\.secretKey

Specify the Secret key of 8 to 40 characters in length that clients use to access the server.
This overrides the secret key that is generated by MinIO on first startup and stored inside the
`configDir` directory.




*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/minio\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/minio.nix)



## services\.mongodb\.enable

Whether to enable MongoDB process and expose utilities.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/mongodb\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mongodb.nix)



## services\.mongodb\.package

Which MongoDB package to use.



*Type:*
package



*Default:*
` pkgs.mongodb `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/mongodb\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mongodb.nix)



## services\.mongodb\.additionalArgs

Additional arguments passed to `mongod`.




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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/mongodb\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mongodb.nix)



## services\.mysql\.enable

Whether to enable MySQL process and expose utilities.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/mysql\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix)



## services\.mysql\.package

Which package of MySQL to use



*Type:*
package



*Default:*
` pkgs.mysql80 `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/mysql\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix)



## services\.mysql\.ensureUsers

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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/mysql\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix)



## services\.mysql\.ensureUsers\.\*\.ensurePermissions

Permissions to ensure for the user, specified as attribute set.
The attribute names specify the database and tables to grant the permissions for,
separated by a dot. You may use wildcards here.
The attribute values specfiy the permissions to grant.
You may specify one or multiple comma-separated SQL privileges here.
For more information on how to specify the target
and on which privileges exist, see the
[GRANT syntax](https://mariadb.com/kb/en/library/grant/).
The attributes are used as `GRANT ${attrName} ON ${attrValue}`.




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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/mysql\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix)



## services\.mysql\.ensureUsers\.\*\.name

Name of the user to ensure.




*Type:*
string

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/mysql\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix)



## services\.mysql\.ensureUsers\.\*\.password

Password of the user to ensure.




*Type:*
null or string



*Default:*
` null `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/mysql\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix)



## services\.mysql\.importTimeZones

Whether to import tzdata on the first startup of the mysql server




*Type:*
null or boolean



*Default:*
` null `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/mysql\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix)



## services\.mysql\.initialDatabases

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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/mysql\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix)



## services\.mysql\.initialDatabases\.\*\.name

The name of the database to create.




*Type:*
string

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/mysql\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix)



## services\.mysql\.initialDatabases\.\*\.schema

The initial schema of the database; if null (the default),
an empty database is created.




*Type:*
null or path



*Default:*
` null `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/mysql\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix)



## services\.mysql\.settings

MySQL configuration.




*Type:*
attribute set of attribute set of (INI atom (null, bool, int, float or string) or a list of them for duplicate keys)



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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/mysql\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/mysql.nix)



## services\.postgres\.enable

Whether to enable Add PostgreSQL process.
.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/postgres\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix)



## services\.postgres\.package

Which version of PostgreSQL to use



*Type:*
package



*Default:*
` pkgs.postgresql `



*Example:*

```
# see https://github.com/NixOS/nixpkgs/blob/master/pkgs/servers/sql/postgresql/packages.nix for full list
pkgs.postgresql_13.withPackages (p: [ p.pg_cron p.timescaledb p.pg_partman ]);

```

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/postgres\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix)



## services\.postgres\.createDatabase

Create a database named like current user on startup. Only applies when initialDatabases is an empty list.




*Type:*
boolean



*Default:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/postgres\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix)



## services\.postgres\.initdbArgs

Additional arguments passed to `initdb` during data dir
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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/postgres\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix)



## services\.postgres\.initialDatabases

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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/postgres\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix)



## services\.postgres\.initialDatabases\.\*\.name

The name of the database to create.




*Type:*
string

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/postgres\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix)



## services\.postgres\.initialDatabases\.\*\.schema

The initial schema of the database; if null (the default),
an empty database is created.




*Type:*
null or path



*Default:*
` null `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/postgres\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix)



## services\.postgres\.initialScript

Initial SQL commands to run during database initialization. This can be multiple
SQL expressions separated by a semi-colon.




*Type:*
null or string



*Default:*
` null `



*Example:*

```
CREATE USER postgres SUPERUSER;
CREATE USER bar;

```

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/postgres\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix)



## services\.postgres\.listen_addresses

Listen address



*Type:*
string



*Default:*
` "" `



*Example:*
` "127.0.0.1" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/postgres\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix)



## services\.postgres\.port

The TCP port to accept connections.




*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*
` 5432 `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/postgres\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix)



## services\.postgres\.settings

PostgreSQL configuration. Refer to
<https://www.postgresql.org/docs/11/config-setting.html#CONFIG-SETTING-CONFIGURATION-FILE>
for an overview of `postgresql.conf`.

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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/postgres\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/postgres.nix)



## services\.rabbitmq\.enable

Whether to enable the RabbitMQ server, an Advanced Message
Queuing Protocol (AMQP) broker.




*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/rabbitmq\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix)



## services\.rabbitmq\.package

Which rabbitmq package to use.




*Type:*
package



*Default:*
` pkgs.rabbitmq-server `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/rabbitmq\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix)



## services\.rabbitmq\.configItems

Configuration options in RabbitMQ's new config file format,
which is a simple key-value format that can not express nested
data structures. This is known as the `rabbitmq.conf` file,
although outside NixOS that filename may have Erlang syntax, particularly
prior to RabbitMQ 3.7.0.
If you do need to express nested data structures, you can use
`config` option. Configuration from `config`
will be merged into these options by RabbitMQ at runtime to
form the final configuration.
See <https://www.rabbitmq.com/configure.html#config-items>
For the distinct formats, see <https://www.rabbitmq.com/configure.html#config-file-formats>




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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/rabbitmq\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix)



## services\.rabbitmq\.cookie

Erlang cookie is a string of arbitrary length which must
be the same for several nodes to be allowed to communicate.
Leave empty to generate automatically.




*Type:*
string



*Default:*
` "" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/rabbitmq\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix)



## services\.rabbitmq\.listenAddress

IP address on which RabbitMQ will listen for AMQP
connections.  Set to the empty string to listen on all
interfaces.  Note that RabbitMQ creates a user named
`guest` with password
`guest` by default, so you should delete
this user if you intend to allow external access.
Together with 'port' setting it's mostly an alias for
configItems."listeners.tcp.1" and it's left for backwards
compatibility with previous version of this module.




*Type:*
string



*Default:*
` "127.0.0.1" `



*Example:*
` "" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/rabbitmq\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix)



## services\.rabbitmq\.managementPlugin\.enable

Whether to enable the management plugin.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/rabbitmq\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix)



## services\.rabbitmq\.managementPlugin\.port

On which port to run the management plugin




*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*
` 15672 `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/rabbitmq\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix)



## services\.rabbitmq\.pluginDirs

The list of directories containing external plugins



*Type:*
list of path



*Default:*
` [ ] `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/rabbitmq\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix)



## services\.rabbitmq\.plugins

The names of plugins to enable



*Type:*
list of string



*Default:*
` [ ] `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/rabbitmq\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix)



## services\.rabbitmq\.port

Port on which RabbitMQ will listen for AMQP connections.




*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*
` 5672 `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/rabbitmq\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/rabbitmq.nix)



## services\.redis\.enable

Whether to enable Redis process and expose utilities.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/redis\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/redis.nix)



## services\.redis\.package

Which package of Redis to use



*Type:*
package



*Default:*
` pkgs.redis `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/redis\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/redis.nix)



## services\.redis\.bind

The IP interface to bind to.
`null` means "all interfaces".




*Type:*
null or string



*Default:*
` "127.0.0.1" `



*Example:*
` "127.0.0.1" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/redis\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/redis.nix)



## services\.redis\.extraConfig

Additional text to be appended to `redis.conf`.



*Type:*
strings concatenated with “\\n”



*Default:*
` "" `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/redis\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/redis.nix)



## services\.redis\.port

The TCP port to accept connections.
If port 0 is specified Redis, will not listen on a TCP socket.




*Type:*
16 bit unsigned integer; between 0 and 65535 (both inclusive)



*Default:*
` 6379 `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/redis\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/redis.nix)



## services\.wiremock\.enable

Whether to enable WireMock.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/wiremock\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/wiremock.nix)



## services\.wiremock\.package

Which package of WireMock to use.




*Type:*
package



*Default:*
` pkgs.wiremock `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/wiremock\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/wiremock.nix)



## services\.wiremock\.disableBanner

Whether to disable print banner logo.




*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/wiremock\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/wiremock.nix)



## services\.wiremock\.mappings

The mappings to mock.
See the JSON examples on <https://wiremock.org/docs/stubbing/> for more information.




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
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/wiremock\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/wiremock.nix)



## services\.wiremock\.port

The port number for the HTTP server to listen on.




*Type:*
signed integer



*Default:*
` 8080 `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/wiremock\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/wiremock.nix)



## services\.wiremock\.verbose

Whether to log verbosely to stdout.




*Type:*
boolean



*Default:*
` false `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/services/wiremock\.nix](https://github.com/cachix/devenv/blob/main/src/modules/services/wiremock.nix)



## starship\.enable

Whether to enable the Starship.rs command prompt.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/integrations/starship\.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/starship.nix)



## starship\.package

The Starship package to use.



*Type:*
package



*Default:*
` pkgs.starship `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/integrations/starship\.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/starship.nix)



## starship\.config\.enable

Whether to enable Starship config override.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/integrations/starship\.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/starship.nix)



## starship\.config\.path

The Starship configuration file to use.



*Type:*
path



*Default:*
` ${config.env.DEVENV_ROOT}/starship.toml `

*Declared by:*
 - [https://github\.com/cachix/devenv/blob/main/src/modules/integrations/starship\.nix](https://github.com/cachix/devenv/blob/main/src/modules/integrations/starship.nix)


