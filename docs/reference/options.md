# devenv.nix options

## devcontainer.enable
Whether to enable generation .devcontainer.json for devenv integration.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## devenv.flakesIntegration
Tells if devenv is being imported by a flake.nix file


*_Type_*
```
boolean
```


*_Default_*
```
false
```




## devenv.latestVersion
The latest version of devenv.


*_Type_*
```
string
```


*_Default_*
```
"0.5"
```




## devenv.warnOnNewVersion
Whether to warn when a new version of devenv is available.


*_Type_*
```
boolean
```


*_Default_*
```
true
```




## difftastic.enable
Integrate difftastic into git: https://difftastic.wilfred.me.uk/

*_Type_*
```
boolean
```


*_Default_*
```
false
```




## enterShell
Bash code to execute when entering the shell.

*_Type_*
```
strings concatenated with "\n"
```


*_Default_*
```
""
```




## env
Environment variables to be exposed inside the developer environment.

*_Type_*
```
lazy attribute set of anything
```


*_Default_*
```
{ }
```




## hosts
List of hosts entries.

*_Type_*
```
attribute set of string
```


*_Default_*
```
{ }
```


*_Example_*
```
{
  "example.com" = "127.0.0.1";
}
```


## hostsProfileName
Profile name to use.

*_Type_*
```
string
```


*_Default_*
```
"devenv-e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
```




## languages.c.enable
Whether to enable tools for C development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.clojure.enable
Whether to enable tools for Clojure development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.cplusplus.enable
Whether to enable tools for C++ development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.crystal.enable
Whether to enable Enable tools for Crystal development..

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.cue.enable
Whether to enable tools for Cue development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.cue.package
The CUE package to use.

*_Type_*
```
package
```


*_Default_*
```
pkgs.cue
```




## languages.dart.enable
Whether to enable tools for Dart development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.dart.package
The Dart package to use.

*_Type_*
```
package
```


*_Default_*
```
pkgs.dart
```




## languages.deno.enable
Whether to enable tools for Deno development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.dotnet.enable
Whether to enable tools for .NET development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.elixir.enable
Whether to enable tools for Elixir development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.elixir.package
Which package of Elixir to use

*_Type_*
```
package
```


*_Default_*
```
pkgs.elixir
```




## languages.elm.enable
Whether to enable tools for Elm development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.erlang.enable
Whether to enable tools for Erlang development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.erlang.package
Which package of Erlang to use

*_Type_*
```
package
```


*_Default_*
```
pkgs.erlang
```




## languages.go.enable
Whether to enable tools for Go development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.haskell.enable
Whether to enable tools for Haskell development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.java.enable
Whether to enable tools for Java development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.java.gradle.enable
Whether to enable gradle.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.java.gradle.package
The gradle package to use.
The gradle package by default inherits the JDK from `languages.java.jdk.package`.


*_Type_*
```
package
```






## languages.java.jdk.package
{'_type': 'mdDoc', 'text': 'The JDK package to use.\nThis will also become available as `JAVA_HOME`.\n'}

*_Type_*
```
package
```


*_Default_*
```
pkgs.jdk
```


*_Example_*
```
pkgs.jdk8
```


## languages.java.maven.enable
Whether to enable maven.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.java.maven.package
{'_type': 'mdDoc', 'text': 'The maven package to use.\nThe maven package by default inherits the JDK from `languages.java.jdk.package`.\n'}

*_Type_*
```
package
```






## languages.javascript.enable
Whether to enable tools for JavaScript development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.javascript.package
The Node package to use.

*_Type_*
```
package
```


*_Default_*
```
pkgs.nodejs
```




## languages.julia.enable
Whether to enable tools for Julia development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.julia.package
The Julia package to use.

*_Type_*
```
package
```


*_Default_*
```
pkgs.julia-bin
```




## languages.kotlin.enable
Whether to enable tools for Kotlin development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.lua.enable
Whether to enable tools for Lua development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.lua.package
The Lua package to use.

*_Type_*
```
package
```


*_Default_*
```
pkgs.lua
```




## languages.nim.enable
Whether to enable tools for nim development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.nim.package
The nim package to use.

*_Type_*
```
package
```


*_Default_*
```
pkgs.nim
```




## languages.nix.enable
Whether to enable tools for Nix development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.ocaml.enable
Whether to enable tools for OCaml development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.perl.enable
Whether to enable tools for Perl development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.php.enable
Whether to enable tools for PHP development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.php.fpm.extraConfig
Extra configuration that should be put in the global section of
the PHP-FPM configuration file. Do not specify the options
<literal>error_log</literal> or
<literal>daemonize</literal> here, since they are generated by
NixOS.


*_Type_*
```
null or strings concatenated with "\n"
```


*_Default_*
```
null
```




## languages.php.fpm.phpOptions
Options appended to the PHP configuration file <filename>php.ini</filename>.


*_Type_*
```
strings concatenated with "\n"
```


*_Default_*
```
""
```


*_Example_*
```
''
  date.timezone = "CET"
''
```


## languages.php.fpm.pools
PHP-FPM pools. If no pools are defined, the PHP-FPM
service is disabled.


*_Type_*
```
attribute set of (submodule)
```


*_Default_*
```
{ }
```


*_Example_*
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


## languages.php.fpm.pools.&lt;name&gt;.extraConfig
Extra lines that go into the pool configuration.
See the documentation on <literal>php-fpm.conf</literal> for
details on configuration directives.


*_Type_*
```
null or strings concatenated with "\n"
```


*_Default_*
```
null
```




## languages.php.fpm.pools.&lt;name&gt;.listen
The address on which to accept FastCGI requests.


*_Type_*
```
string
```


*_Default_*
```
""
```


*_Example_*
```
"/path/to/unix/socket"
```


## languages.php.fpm.pools.&lt;name&gt;.phpEnv
Environment variables used for this PHP-FPM pool.


*_Type_*
```
attribute set of string
```


*_Default_*
```
{ }
```


*_Example_*
```
{
  HOSTNAME = "$HOSTNAME";
  TMP = "/tmp";
  TMPDIR = "/tmp";
  TEMP = "/tmp";
}

```


## languages.php.fpm.pools.&lt;name&gt;.phpOptions
"Options appended to the PHP configuration file <filename>php.ini</filename> used for this PHP-FPM pool."


*_Type_*
```
strings concatenated with "\n"
```






## languages.php.fpm.pools.&lt;name&gt;.phpPackage
The PHP package to use for running this PHP-FPM pool.


*_Type_*
```
package
```


*_Default_*
```
phpfpm.phpPackage
```




## languages.php.fpm.pools.&lt;name&gt;.settings
PHP-FPM pool directives. Refer to the "List of pool directives" section of
<link xlink:href="https://www.php.net/manual/en/install.fpm.configuration.php"/>
for details. Note that settings names must be enclosed in quotes (e.g.
<literal>"pm.max_children"</literal> instead of <literal>pm.max_children</literal>).


*_Type_*
```
attribute set of (string or signed integer or boolean)
```


*_Default_*
```
{ }
```


*_Example_*
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


## languages.php.fpm.pools.&lt;name&gt;.socket
Path to the unix socket file on which to accept FastCGI requests.
<note><para>This option is read-only and managed by NixOS.</para></note>


*_Type_*
```
string
```




*_Example_*
```
"/tmp/<name>.sock"
```


## languages.php.fpm.settings
PHP-FPM global directives. Refer to the "List of global php-fpm.conf directives" section of
<link xlink:href="https://www.php.net/manual/en/install.fpm.configuration.php"/>
for details. Note that settings names must be enclosed in quotes (e.g.
<literal>"pm.max_children"</literal> instead of <literal>pm.max_children</literal>).
You need not specify the options <literal>error_log</literal> or
<literal>daemonize</literal> here, since they are generated by NixOS.


*_Type_*
```
attribute set of (string or signed integer or boolean)
```


*_Default_*
```
{
  error_log = "/.devenv/state/php-fpm/php-fpm.log";
}
```




## languages.php.package
{'_type': 'mdDoc', 'text': "Allows to [override the default used package](https://nixos.org/manual/nixpkgs/stable/#ssec-php-user-guide) to adjust the settings or add more extensions. You can find the extensions using `devenv search 'php extensions'`\n```\n"}

*_Type_*
```
package
```


*_Default_*
```
pkgs.php
```


*_Example_*
```
pkgs.php.buildEnv {
  extensions = { all, enabled }: with all; enabled ++ [ xdebug ];
  extraConfig = ''
    memory_limit=1G
  '';
};

```


## languages.purescript.enable
Whether to enable tools for PureScript development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.purescript.package
The PureScript package to use.

*_Type_*
```
package
```


*_Default_*
```
pkgs.purescript
```




## languages.python.enable
Whether to enable tools for Python development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.python.package
The Python package to use.

*_Type_*
```
package
```


*_Default_*
```
pkgs.python3
```




## languages.python.poetry.enable
Whether to enable poetry.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.python.poetry.package
The poetry package to use.

*_Type_*
```
package
```


*_Default_*
```
config.languages.python.package.pkgs.poetry
```




## languages.python.venv.enable
Whether to enable Python virtual environment.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.r.enable
Whether to enable tools for R development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.r.package
The R package to use.

*_Type_*
```
package
```


*_Default_*
```
pkgs.R
```




## languages.racket.enable
Whether to enable tools for Racket development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.racket.package
The Racket package to use.

*_Type_*
```
package
```


*_Default_*
```
pkgs.racket-minimal
```




## languages.robotframework.enable
Whether to enable tools for Robot Framework development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.robotframework.python
The Python package to use.

*_Type_*
```
package
```


*_Default_*
```
pkgs.python3
```




## languages.ruby.enable
Whether to enable tools for Ruby development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.ruby.package
The Ruby package to use.

*_Type_*
```
package
```


*_Default_*
```
pkgs.ruby_3_1
```




## languages.rust.enable
Whether to enable tools for Rust development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.rust.packages
Attribute set of packages including rustc and cargo

*_Type_*
```
submodule
```


*_Default_*
```
pkgs
```




## languages.rust.packages.cargo
cargo package

*_Type_*
```
package
```


*_Default_*
```
pkgs.cargo
```




## languages.rust.packages.clippy
clippy package

*_Type_*
```
package
```


*_Default_*
```
pkgs.clippy
```




## languages.rust.packages.rust-analyzer
rust-analyzer package

*_Type_*
```
package
```


*_Default_*
```
pkgs.rust-analyzer
```




## languages.rust.packages.rust-src
rust-src package

*_Type_*
```
package or string
```


*_Default_*
```
pkgs.rustPlatform.rustLibSrc
```




## languages.rust.packages.rustc
rustc package

*_Type_*
```
package
```


*_Default_*
```
pkgs.rustc
```




## languages.rust.packages.rustfmt
rustfmt package

*_Type_*
```
package
```


*_Default_*
```
pkgs.rustfmt
```




## languages.rust.version
Set to stable, beta or latest.

*_Type_*
```
null or string
```


*_Default_*
```
null
```




## languages.scala.enable
Whether to enable tools for Scala development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.scala.package
The Scala package to use.


*_Type_*
```
package
```


*_Default_*
```
"pkgs.scala_3"
```




## languages.terraform.enable
Whether to enable tools for terraform development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.terraform.package
The terraform package to use.

*_Type_*
```
package
```


*_Default_*
```
pkgs.terraform
```




## languages.typescript.enable
Whether to enable tools for TypeScript development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.unison.enable
Whether to enable tools for Unison development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.unison.package
Which package of Unison to use

*_Type_*
```
package
```


*_Default_*
```
pkgs.unison-ucm
```




## languages.v.enable
Whether to enable tools for v development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.v.package
The v package to use.

*_Type_*
```
package
```


*_Default_*
```
pkgs.vlang
```




## languages.zig.enable
Whether to enable tools for Zig development.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## languages.zig.package
Which package of Zig to use

*_Type_*
```
package
```


*_Default_*
```
pkgs.zig
```




## packages
A list of packages to expose inside the developer environment. Search available packages using ``devenv search NAME``.

*_Type_*
```
list of package
```


*_Default_*
```
[ ]
```




## pre-commit
Integration of https://github.com/cachix/pre-commit-hooks.nix

*_Type_*
```
submodule
```


*_Default_*
```
{ }
```




## pre-commit.default_stages
{'_type': 'mdDoc', 'text': 'A configuration wide option for the stages property.\nInstalls hooks to the defined stages.\nSee [https://pre-commit.com/#confining-hooks-to-run-at-certain-stages](https://pre-commit.com/#confining-hooks-to-run-at-certain-stages).\n'}

*_Type_*
```
list of string
```


*_Default_*
```
[
  "commit"
]
```




## pre-commit.excludes
{'_type': 'mdDoc', 'text': 'Exclude files that were matched by these patterns.\n'}

*_Type_*
```
list of string
```


*_Default_*
```
[ ]
```




## pre-commit.hooks
{'_type': 'mdDoc', 'text': 'The hook definitions.\n\nPre-defined hooks can be enabled by, for example:\n\n```nix\nhooks.nixpkgs-fmt.enable = true;\n```\n\nThe pre-defined hooks are:\n\n**`actionlint`**\n\nStatic checker for GitHub Actions workflow files.\n\n\n**`alejandra`**\n\nThe Uncompromising Nix Code Formatter.\n\n\n**`ansible-lint`**\n\nAnsible linter.\n\n\n**`autoflake`**\n\nRemove unused imports and variables from Python code.\n\n\n**`bats`**\n\nRun bash unit tests.\n\n\n**`black`**\n\nThe uncompromising Python code formatter.\n\n\n**`cabal-fmt`**\n\nFormat Cabal files\n\n\n**`cabal2nix`**\n\nRun `cabal2nix` on all `*.cabal` files to generate corresponding `default.nix` files.\n\n\n**`cargo-check`**\n\nCheck the cargo package for errors.\n\n\n**`chktex`**\n\nLaTeX semantic checker\n\n\n**`clang-format`**\n\nFormat your code using `clang-format`.\n\n\n**`clippy`**\n\nLint Rust code.\n\n\n**`commitizen`**\n\nCheck whether the current commit message follows commiting rules.\n\n\n\n**`deadnix`**\n\nScan Nix files for dead code (unused variable bindings).\n\n\n**`dhall-format`**\n\nDhall code formatter.\n\n\n**`editorconfig-checker`**\n\nVerify that the files are in harmony with the `.editorconfig`.\n\n\n**`elm-format`**\n\nFormat Elm files.\n\n\n**`elm-review`**\n\nAnalyzes Elm projects, to help find mistakes before your users find them.\n\n\n**`elm-test`**\n\nRun unit tests and fuzz tests for Elm code.\n\n\n**`eslint`**\n\nFind and fix problems in your JavaScript code.\n\n\n**`flake8`**\n\nCheck the style and quality of Python files.\n\n\n**`fourmolu`**\n\nHaskell code prettifier.\n\n\n**`gofmt`**\n\nA tool that automatically formats Go source code\n\n\n**`gotest`**\n\nRun go tests\n\n\n**`govet`**\n\nChecks correctness of Go programs.\n\n\n**`hadolint`**\n\nDockerfile linter, validate inline bash.\n\n\n**`hindent`**\n\nHaskell code prettifier.\n\n\n**`hlint`**\n\nHLint gives suggestions on how to improve your source code.\n\n\n**`hpack`**\n\n`hpack` converts package definitions in the hpack format (`package.yaml`) to Cabal files.\n\n\n**`html-tidy`**\n\nHTML linter.\n\n\n**`hunspell`**\n\nSpell checker and morphological analyzer.\n\n\n**`isort`**\n\nA Python utility / library to sort imports.\n\n\n**`latexindent`**\n\nPerl script to add indentation to LaTeX files.\n\n\n**`luacheck`**\n\nA tool for linting and static analysis of Lua code.\n\n\n**`markdownlint`**\n\nStyle checker and linter for markdown files.\n\n\n**`mdsh`**\n\nMarkdown shell pre-processor.\n\n\n**`nixfmt`**\n\nNix code prettifier.\n\n\n**`nixpkgs-fmt`**\n\nNix code prettifier.\n\n\n**`ormolu`**\n\nHaskell code prettifier.\n\n\n**`php-cs-fixer`**\n\nLint PHP files.\n\n\n**`phpcbf`**\n\nLint PHP files.\n\n\n**`phpcs`**\n\nLint PHP files.\n\n\n**`prettier`**\n\nOpinionated multi-language code formatter.\n\n\n**`purs-tidy`**\n\nFormat purescript files.\n\n\n**`purty`**\n\nFormat purescript files.\n\n\n**`pylint`**\n\nLint Python files.\n\n\n**`revive`**\n\nA linter for Go source code.\n\n\n**`ruff`**\n\n An extremely fast Python linter, written in Rust.\n\n\n**`rustfmt`**\n\nFormat Rust code.\n\n\n**`shellcheck`**\n\nFormat shell files.\n\n\n**`shfmt`**\n\nFormat shell files.\n\n\n**`staticcheck`**\n\nState of the art linter for the Go programming language\n\n\n**`statix`**\n\nLints and suggestions for the Nix programming language.\n\n\n**`stylish-haskell`**\n\nA simple Haskell code prettifier\n\n\n**`stylua`**\n\nAn Opinionated Lua Code Formatter.\n\n\n**`terraform-format`**\n\nFormat terraform (`.tf`) files.\n\n\n**`typos`**\n\nSource code spell checker\n\n\n**`yamllint`**\n\nYaml linter.\n\n\n'}

*_Type_*
```
attribute set of (submodule)
```


*_Default_*
```
{ }
```




## pre-commit.hooks.&lt;name&gt;.description
{'_type': 'mdDoc', 'text': 'Description of the hook. used for metadata purposes only.\n'}

*_Type_*
```
string
```


*_Default_*
```
""
```




## pre-commit.hooks.&lt;name&gt;.enable
{'_type': 'mdDoc', 'text': 'Whether to enable this pre-commit hook.'}

*_Type_*
```
boolean
```


*_Default_*
```
false
```




## pre-commit.hooks.&lt;name&gt;.entry
{'_type': 'mdDoc', 'text': 'The entry point - the executable to run. {option}`entry` can also contain arguments that will not be overridden, such as `entry = "autopep8 -i";`.\n'}

*_Type_*
```
string
```






## pre-commit.hooks.&lt;name&gt;.excludes
{'_type': 'mdDoc', 'text': 'Exclude files that were matched by these patterns.\n'}

*_Type_*
```
list of string
```


*_Default_*
```
[ ]
```




## pre-commit.hooks.&lt;name&gt;.files
{'_type': 'mdDoc', 'text': 'The pattern of files to run on.\n'}

*_Type_*
```
string
```


*_Default_*
```
""
```




## pre-commit.hooks.&lt;name&gt;.language
{'_type': 'mdDoc', 'text': 'The language of the hook - tells pre-commit how to install the hook.\n'}

*_Type_*
```
string
```


*_Default_*
```
"system"
```




## pre-commit.hooks.&lt;name&gt;.name
{'_type': 'mdDoc', 'text': 'The name of the hook - shown during hook execution.\n'}

*_Type_*
```
string
```


*_Default_*
```
internal name, same as id
```




## pre-commit.hooks.&lt;name&gt;.pass_filenames
{'_type': 'mdDoc', 'text': 'Whether to pass filenames as arguments to the entry point.\n'}

*_Type_*
```
boolean
```


*_Default_*
```
true
```




## pre-commit.hooks.&lt;name&gt;.raw
{'_type': 'mdDoc', 'text': 'Raw fields of a pre-commit hook. This is mostly for internal use but\nexposed in case you need to work around something.\n\nDefault: taken from the other hook options.\n'}

*_Type_*
```
attribute set of unspecified value
```






## pre-commit.hooks.&lt;name&gt;.stages
{'_type': 'mdDoc', 'text': 'Confines the hook to run at a particular stage.\n'}

*_Type_*
```
list of string
```


*_Default_*
```
default_stages
```




## pre-commit.hooks.&lt;name&gt;.types
{'_type': 'mdDoc', 'text': 'List of file types to run on. See [Filtering files with types](https://pre-commit.com/#plugins).\n'}

*_Type_*
```
list of string
```


*_Default_*
```
[
  "file"
]
```




## pre-commit.hooks.&lt;name&gt;.types_or
{'_type': 'mdDoc', 'text': 'List of file types to run on, where only a single type needs to match.\n'}

*_Type_*
```
list of string
```


*_Default_*
```
[ ]
```




## pre-commit.installationScript
{'_type': 'mdDoc', 'text': 'A bash snippet that installs nix-pre-commit-hooks in the current directory\n'}

*_Type_*
```
string
```






## pre-commit.package
{'_type': 'mdDoc', 'text': 'The `pre-commit` package to use.\n'}

*_Type_*
```
package
```






## pre-commit.rootSrc
{'_type': 'mdDoc', 'text': 'The source of the project to be checked.\n\nThis is used in the derivation that performs the check.\n\nIf you use the `flakeModule`, the default is `self.outPath`; the whole flake\nsources.\n'}

*_Type_*
```
path
```






## pre-commit.run
{'_type': 'mdDoc', 'text': 'A derivation that tests whether the pre-commit hooks run cleanly on\nthe entire project.\n'}

*_Type_*
```
package
```


*_Default_*
```
"<derivation>"
```




## pre-commit.settings.alejandra.exclude
{'_type': 'mdDoc', 'text': 'Files or directories to exclude from formatting.'}

*_Type_*
```
list of string
```


*_Default_*
```
[ ]
```


*_Example_*
```
[
  "flake.nix"
  "./templates"
]
```


## pre-commit.settings.autoflake.binPath
{'_type': 'mdDoc', 'text': 'Path to autoflake binary.'}

*_Type_*
```
string
```


*_Default_*
```
"${pkgs.autoflake}/bin/autoflake"

```




## pre-commit.settings.autoflake.flags
{'_type': 'mdDoc', 'text': 'Flags passed to autoflake.'}

*_Type_*
```
string
```


*_Default_*
```
"--in-place --expand-star-imports --remove-duplicate-keys --remove-unused-variables"
```




## pre-commit.settings.clippy.denyWarnings
{'_type': 'mdDoc', 'text': 'Fail when warnings are present'}

*_Type_*
```
boolean
```


*_Default_*
```
false
```




## pre-commit.settings.deadnix.edit
{'_type': 'mdDoc', 'text': 'Remove unused code and write to source file.'}

*_Type_*
```
boolean
```


*_Default_*
```
false
```




## pre-commit.settings.deadnix.noLambdaArg
{'_type': 'mdDoc', 'text': "Don't check lambda parameter arguments."}

*_Type_*
```
boolean
```


*_Default_*
```
false
```




## pre-commit.settings.deadnix.noLambdaPatternNames
{'_type': 'mdDoc', 'text': "Don't check lambda pattern names (don't break nixpkgs `callPackage`)."}

*_Type_*
```
boolean
```


*_Default_*
```
false
```




## pre-commit.settings.deadnix.noUnderscore
{'_type': 'mdDoc', 'text': "Don't check any bindings that start with a `_`."}

*_Type_*
```
boolean
```


*_Default_*
```
false
```




## pre-commit.settings.deadnix.quiet
{'_type': 'mdDoc', 'text': "Don't print a dead code report."}

*_Type_*
```
boolean
```


*_Default_*
```
false
```




## pre-commit.settings.eslint.binPath
{'_type': 'mdDoc', 'text': '`eslint` binary path. E.g. if you want to use the `eslint` in `node_modules`, use `./node_modules/.bin/eslint`.'}

*_Type_*
```
path
```


*_Default_*
```
${tools.eslint}/bin/eslint
```




## pre-commit.settings.eslint.extensions
{'_type': 'mdDoc', 'text': 'The pattern of files to run on, see [https://pre-commit.com/#hooks-files](https://pre-commit.com/#hooks-files).'}

*_Type_*
```
string
```


*_Default_*
```
"\\.js$"
```




## pre-commit.settings.flake8.binPath
{'_type': 'mdDoc', 'text': 'flake8 binary path. Should be used to specify flake8 binary from your Nix-managed Python environment.'}

*_Type_*
```
string
```


*_Default_*
```
"${pkgs.python39Packages.flake8}/bin/flake8"

```




## pre-commit.settings.flake8.format
{'_type': 'mdDoc', 'text': 'Output format.'}

*_Type_*
```
string
```


*_Default_*
```
"default"
```




## pre-commit.settings.hpack.silent
{'_type': 'mdDoc', 'text': 'Whether generation should be silent.'}

*_Type_*
```
boolean
```


*_Default_*
```
false
```




## pre-commit.settings.markdownlint.config
{'_type': 'mdDoc', 'text': 'See https://github.com/DavidAnson/markdownlint/blob/main/schema/.markdownlint.jsonc'}

*_Type_*
```
attribute set
```


*_Default_*
```
{ }
```




## pre-commit.settings.nixfmt.width
{'_type': 'mdDoc', 'text': 'Line width.'}

*_Type_*
```
null or signed integer
```


*_Default_*
```
null
```




## pre-commit.settings.ormolu.cabalDefaultExtensions
{'_type': 'mdDoc', 'text': 'Use `default-extensions` from `.cabal` files.'}

*_Type_*
```
boolean
```


*_Default_*
```
false
```




## pre-commit.settings.ormolu.defaultExtensions
{'_type': 'mdDoc', 'text': 'Haskell language extensions to enable.'}

*_Type_*
```
list of string
```


*_Default_*
```
[ ]
```




## pre-commit.settings.php-cs-fixer.binPath
{'_type': 'mdDoc', 'text': 'PHP-CS-Fixer binary path.'}

*_Type_*
```
string
```


*_Default_*
```
"${pkgs.php81Packages.php-cs-fixer}/bin/php-cs-fixer"

```




## pre-commit.settings.phpcbf.binPath
{'_type': 'mdDoc', 'text': 'PHP_CodeSniffer binary path.'}

*_Type_*
```
string
```


*_Default_*
```
"${pkgs.php80Packages.phpcbf}/bin/phpcbf"

```




## pre-commit.settings.phpcs.binPath
{'_type': 'mdDoc', 'text': 'PHP_CodeSniffer binary path.'}

*_Type_*
```
string
```


*_Default_*
```
"${pkgs.php80Packages.phpcs}/bin/phpcs"

```




## pre-commit.settings.prettier.binPath
{'_type': 'mdDoc', 'text': '`prettier` binary path. E.g. if you want to use the `prettier` in `node_modules`, use `./node_modules/.bin/prettier`.'}

*_Type_*
```
path
```


*_Default_*
```
"${tools.prettier}/bin/prettier"

```




## pre-commit.settings.prettier.output
{'_type': 'mdDoc', 'text': 'Output format.'}

*_Type_*
```
null or one of "check", "list-different"
```


*_Default_*
```
"list-different"
```




## pre-commit.settings.prettier.write
{'_type': 'mdDoc', 'text': 'Whether to edit files inplace.'}

*_Type_*
```
boolean
```


*_Default_*
```
true
```




## pre-commit.settings.pylint.binPath
{'_type': 'mdDoc', 'text': 'Pylint binary path. Should be used to specify Pylint binary from your Nix-managed Python environment.'}

*_Type_*
```
string
```


*_Default_*
```
"${pkgs.python39Packages.pylint}/bin/pylint"

```




## pre-commit.settings.pylint.reports
{'_type': 'mdDoc', 'text': 'Whether to display a full report.'}

*_Type_*
```
boolean
```


*_Default_*
```
false
```




## pre-commit.settings.pylint.score
{'_type': 'mdDoc', 'text': 'Whether to activate the evaluation score.'}

*_Type_*
```
boolean
```


*_Default_*
```
true
```




## pre-commit.settings.revive.configPath
{'_type': 'mdDoc', 'text': 'Path to the configuration TOML file.'}

*_Type_*
```
string
```


*_Default_*
```
""
```




## pre-commit.settings.rust.cargoManifestPath
{'_type': 'mdDoc', 'text': 'Path to Cargo.toml'}

*_Type_*
```
null or string
```


*_Default_*
```
null
```




## pre-commit.settings.statix.format
{'_type': 'mdDoc', 'text': 'Error Output format.'}

*_Type_*
```
one of "stderr", "errfmt", "json"
```


*_Default_*
```
"errfmt"
```




## pre-commit.settings.statix.ignore
{'_type': 'mdDoc', 'text': 'Globs of file patterns to skip.'}

*_Type_*
```
list of string
```


*_Default_*
```
[ ]
```


*_Example_*
```
[
  "flake.nix"
  "_*"
]
```


## pre-commit.settings.typos.diff
{'_type': 'mdDoc', 'text': 'Wheter to print a diff of what would change.'}

*_Type_*
```
boolean
```


*_Default_*
```
false
```




## pre-commit.settings.typos.format
{'_type': 'mdDoc', 'text': 'Output format.'}

*_Type_*
```
one of "silent", "brief", "long", "json"
```


*_Default_*
```
"long"
```




## pre-commit.settings.typos.write
{'_type': 'mdDoc', 'text': 'Whether to write fixes out.'}

*_Type_*
```
boolean
```


*_Default_*
```
false
```




## pre-commit.settings.yamllint.configPath
path to the configuration YAML file

*_Type_*
```
string
```


*_Default_*
```
""
```




## pre-commit.settings.yamllint.relaxed
{'_type': 'mdDoc', 'text': 'Use the relaxed configuration'}

*_Type_*
```
boolean
```


*_Default_*
```
false
```




## pre-commit.src
{'_type': 'mdDoc', 'text': 'Root of the project. By default this will be filtered with the `gitignoreSource`\nfunction later, unless `rootSrc` is specified.\n\nIf you use the `flakeModule`, the default is `self.outPath`; the whole flake\nsources.\n'}

*_Type_*
```
path
```






## pre-commit.tools
{'_type': 'mdDoc', 'text': 'Tool set from which `nix-pre-commit-hooks` will pick binaries.\n\n`nix-pre-commit-hooks` comes with its own set of packages for this purpose.\n'}

*_Type_*
```
lazy attribute set of package
```






## process.after
Bash code to execute after stopping processes.

*_Type_*
```
strings concatenated with "\n"
```


*_Default_*
```
""
```




## process.before
Bash code to execute before starting processes.

*_Type_*
```
strings concatenated with "\n"
```


*_Default_*
```
""
```




## process.implementation
The implementation used when performing ``devenv up``.

*_Type_*
```
one of "honcho", "overmind", "process-compose", "hivemind"
```


*_Default_*
```
"honcho"
```


*_Example_*
```
"overmind"
```


## process.process-compose
Top-level process-compose.yaml options when that implementation is used.


*_Type_*
```
attribute set
```


*_Default_*
```
{
  port = 9999;
  tui = true;
  version = "0.5";
}
```


*_Example_*
```
{
  log_level = "fatal";
  log_location = "/path/to/combined/output/logfile.log";
  version = "0.5";
}
```


## processes
Processes can be started with ``devenv up`` and run in foreground mode.

*_Type_*
```
attribute set of (submodule)
```


*_Default_*
```
{ }
```




## processes.&lt;name&gt;.exec
Bash code to run the process.

*_Type_*
```
string
```






## processes.&lt;name&gt;.process-compose
process-compose.yaml specific process attributes.

Example: https://github.com/F1bonacc1/process-compose/blob/main/process-compose.yaml`

Only used when using ``process.implementation = "process-compose";``


*_Type_*
```
attribute set
```


*_Default_*
```
{ }
```


*_Example_*
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


## scripts
A set of scripts available when the environment is active.

*_Type_*
```
attribute set of (submodule)
```


*_Default_*
```
{ }
```




## scripts.&lt;name&gt;.exec
Bash code to execute when the script is ran.

*_Type_*
```
string
```






## services.adminer.enable
Whether to enable adminer process.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## services.adminer.listen
Listen address for adminer.

*_Type_*
```
string
```


*_Default_*
```
"127.0.0.1:8080"
```




## services.adminer.package
Which package of adminer to use

*_Type_*
```
package
```


*_Default_*
```
pkgs.adminer
```




## services.blackfire.client-id
{'_type': 'mdDoc', 'text': 'Sets the client id used to authenticate with Blackfire\nYou can find your personal client-id at https://blackfire.io/my/settings/credentials\n'}

*_Type_*
```
string
```


*_Default_*
```
""
```




## services.blackfire.client-token
{'_type': 'mdDoc', 'text': 'Sets the client token used to authenticate with Blackfire\nYou can find your personal client-token at https://blackfire.io/my/settings/credentials\n'}

*_Type_*
```
string
```


*_Default_*
```
""
```




## services.blackfire.enable
{'_type': 'mdDoc', 'text': 'Whether to enable Blackfire profiler agent\n\nFor PHP you need to install and configure the Blackfire PHP extension.\n\n```nix\nlanguages.php.package = pkgs.php.buildEnv {\n  extensions = { all, enabled }: with all; enabled ++ [ (blackfire// { extensionName = "blackfire"; }) ];\n  extraConfig = \'\'\n    memory_limit = 256M\n    blackfire.agent_socket = "tcp://127.0.0.1:8307";\n  \'\';\n};\n```\n.'}

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## services.blackfire.package
Which package of blackfire to use

*_Type_*
```
package
```


*_Default_*
```
pkgs.blackfire
```




## services.blackfire.server-id
{'_type': 'mdDoc', 'text': 'Sets the server id used to authenticate with Blackfire\nYou can find your personal server-id at https://blackfire.io/my/settings/credentials\n'}

*_Type_*
```
string
```


*_Default_*
```
""
```




## services.blackfire.server-token
{'_type': 'mdDoc', 'text': 'Sets the server token used to authenticate with Blackfire\nYou can find your personal server-token at https://blackfire.io/my/settings/credentials\n'}

*_Type_*
```
string
```


*_Default_*
```
""
```




## services.blackfire.socket
{'_type': 'mdDoc', 'text': 'Sets the server socket path\n'}

*_Type_*
```
string
```


*_Default_*
```
"tcp://127.0.0.1:8307"
```




## services.caddy.adapter
Name of the config adapter to use.
See https://caddyserver.com/docs/config-adapters for the full list.


*_Type_*
```
string
```


*_Default_*
```
"caddyfile"
```


*_Example_*
```
"nginx"
```


## services.caddy.ca
Certificate authority ACME server. The default (Let's Encrypt
production server) should be fine for most people. Set it to null if
you don't want to include any authority (or if you want to write a more
fine-graned configuration manually)


*_Type_*
```
null or string
```


*_Default_*
```
"https://acme-v02.api.letsencrypt.org/directory"
```


*_Example_*
```
"https://acme-staging-v02.api.letsencrypt.org/directory"
```


## services.caddy.config
Verbatim Caddyfile to use.
Caddy v2 supports multiple config formats via adapters (see <option>services.caddy.adapter</option>).


*_Type_*
```
strings concatenated with "\n"
```


*_Default_*
```
""
```


*_Example_*
```
''
  example.com {
    encode gzip
    log
    root /srv/http
  }
''
```


## services.caddy.dataDir
The data directory, for storing certificates. Before 17.09, this
would create a .caddy directory. With 17.09 the contents of the
.caddy directory are in the specified data directory instead.
Caddy v2 replaced CADDYPATH with XDG directories.
See https://caddyserver.com/docs/conventions#file-locations.


*_Type_*
```
path
```


*_Default_*
```
"/.devenv/state/caddy"
```




## services.caddy.email
Email address (for Let's Encrypt certificate)

*_Type_*
```
string
```


*_Default_*
```
""
```




## services.caddy.enable
Whether to enable Caddy web server.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## services.caddy.package
Caddy package to use.


*_Type_*
```
package
```


*_Default_*
```
pkgs.caddy
```




## services.caddy.resume
Use saved config, if any (and prefer over configuration passed with <option>caddy.config</option>).


*_Type_*
```
boolean
```


*_Default_*
```
false
```




## services.caddy.virtualHosts
Declarative vhost config

*_Type_*
```
attribute set of (submodule)
```


*_Default_*
```
{ }
```


*_Example_*
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


## services.caddy.virtualHosts.&lt;name&gt;.extraConfig
These lines go into the vhost verbatim


*_Type_*
```
strings concatenated with "\n"
```


*_Default_*
```
""
```




## services.caddy.virtualHosts.&lt;name&gt;.serverAliases
Additional names of virtual hosts served by this virtual host configuration.


*_Type_*
```
list of string
```


*_Default_*
```
[ ]
```


*_Example_*
```
[
  "www.example.org"
  "example.org"
]
```


## services.elasticsearch.cluster_name
Elasticsearch name that identifies your cluster for auto-discovery.

*_Type_*
```
string
```


*_Default_*
```
"elasticsearch"
```




## services.elasticsearch.enable
Whether to enable elasticsearch.

*_Type_*
```
boolean
```


*_Default_*
```
false
```




## services.elasticsearch.extraCmdLineOptions
Extra command line options for the elasticsearch launcher.

*_Type_*
```
list of string
```


*_Default_*
```
[ ]
```




## services.elasticsearch.extraConf
Extra configuration for elasticsearch.

*_Type_*
```
string
```


*_Default_*
```
""
```


*_Example_*
```
''
  node.name: "elasticsearch"
  node.master: true
  node.data: false
''
```


## services.elasticsearch.extraJavaOptions
Extra command line options for Java.

*_Type_*
```
list of string
```


*_Default_*
```
[ ]
```


*_Example_*
```
[
  "-Djava.net.preferIPv4Stack=true"
]
```


## services.elasticsearch.listenAddress
Elasticsearch listen address.

*_Type_*
```
string
```


*_Default_*
```
"127.0.0.1"
```




## services.elasticsearch.logging
Elasticsearch logging configuration.

*_Type_*
```
string
```


*_Default_*
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




## services.elasticsearch.package
Elasticsearch package to use.

*_Type_*
```
package
```


*_Default_*
```
pkgs.elasticsearch7
```




## services.elasticsearch.plugins
Extra elasticsearch plugins

*_Type_*
```
list of package
```


*_Default_*
```
[ ]
```


*_Example_*
```
[ pkgs.elasticsearchPlugins.discovery-ec2 ]
```


## services.elasticsearch.port
Elasticsearch port to listen for HTTP traffic.

*_Type_*
```
signed integer
```


*_Default_*
```
9200
```




## services.elasticsearch.single_node
Start a single-node cluster

*_Type_*
```
boolean
```


*_Default_*
```
true
```




## services.elasticsearch.tcp_port
Elasticsearch port for the node to node communication.

*_Type_*
```
signed integer
```


*_Default_*
```
9300
```




## services.mailhog.additionalArgs
Additional arguments passed to `mailhog`.


*_Type_*
```
list of strings concatenated with "\n"
```


*_Default_*
```
[ ]
```


*_Example_*
```
[
  "-invite-jim"
]
```


## services.mailhog.apiListenAddress
Listen address for API.

*_Type_*
```
string
```


*_Default_*
```
"127.0.0.1:8025"
```




## services.mailhog.enable
Whether to enable mailhog process.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## services.mailhog.package
Which package of mailhog to use

*_Type_*
```
package
```


*_Default_*
```
pkgs.mailhog
```




## services.mailhog.smtpListenAddress
Listen address for SMTP.

*_Type_*
```
string
```


*_Default_*
```
"127.0.0.1:1025"
```




## services.mailhog.uiListenAddress
Listen address for UI.

*_Type_*
```
string
```


*_Default_*
```
"127.0.0.1:8025"
```




## services.memcached.bind
{'_type': 'mdDoc', 'text': 'The IP interface to bind to.\n`null` means "all interfaces".\n'}

*_Type_*
```
null or string
```


*_Default_*
```
"127.0.0.1"
```


*_Example_*
```
"127.0.0.1"
```


## services.memcached.enable
Whether to enable memcached process.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## services.memcached.package
Which package of memcached to use

*_Type_*
```
package
```


*_Default_*
```
pkgs.memcached
```




## services.memcached.port
The TCP port to accept connections.
If port 0 is specified Redis will not listen on a TCP socket.


*_Type_*
```
16 bit unsigned integer; between 0 and 65535 (both inclusive)
```


*_Default_*
```
11211
```




## services.memcached.startArgs
Additional arguments passed to `memcached` during startup.


*_Type_*
```
list of strings concatenated with "\n"
```


*_Default_*
```
[ ]
```


*_Example_*
```
[
  "--memory-limit=100M"
]
```


## services.minio.accessKey
{'_type': 'mdDoc', 'text': 'Access key of 5 to 20 characters in length that clients use to access the server.\nThis overrides the access key that is generated by minio on first startup and stored inside the\n`configDir` directory.\n'}

*_Type_*
```
string
```


*_Default_*
```
""
```




## services.minio.browser
{'_type': 'mdDoc', 'text': 'Enable or disable access to web UI.'}

*_Type_*
```
boolean
```


*_Default_*
```
true
```




## services.minio.buckets
{'_type': 'mdDoc', 'text': 'List of buckets to ensure exist on startup.\n'}

*_Type_*
```
list of string
```


*_Default_*
```
[ ]
```




## services.minio.consoleAddress
{'_type': 'mdDoc', 'text': 'IP address and port of the web UI (console).'}

*_Type_*
```
string
```


*_Default_*
```
"127.0.0.1:9001"
```




## services.minio.enable
{'_type': 'mdDoc', 'text': 'Whether to enable Minio Object Storage.'}

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## services.minio.listenAddress
{'_type': 'mdDoc', 'text': 'IP address and port of the server.'}

*_Type_*
```
string
```


*_Default_*
```
"127.0.0.1:9000"
```




## services.minio.package
{'_type': 'mdDoc', 'text': 'Minio package to use.'}

*_Type_*
```
package
```


*_Default_*
```
pkgs.minio
```




## services.minio.region
{'_type': 'mdDoc', 'text': "The physical location of the server. By default it is set to us-east-1, which is same as AWS S3's and Minio's default region.\n"}

*_Type_*
```
string
```


*_Default_*
```
"us-east-1"
```




## services.minio.secretKey
{'_type': 'mdDoc', 'text': 'Specify the Secret key of 8 to 40 characters in length that clients use to access the server.\nThis overrides the secret key that is generated by minio on first startup and stored inside the\n`configDir` directory.\n'}

*_Type_*
```
string
```


*_Default_*
```
""
```




## services.mongodb.additionalArgs
Additional arguments passed to `mongod`.


*_Type_*
```
list of strings concatenated with "\n"
```


*_Default_*
```
[
  "--noauth"
]
```


*_Example_*
```
[
  "--port"
  "27017"
  "--noauth"
]
```


## services.mongodb.enable
Whether to enable MongoDB process and expose utilities.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## services.mongodb.package
Which MongoDB package to use.

*_Type_*
```
package
```


*_Default_*
```
pkgs.mongodb
```




## services.mysql.enable
Whether to enable mysql process and expose utilities.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## services.mysql.ensureUsers
{'_type': 'mdDoc', 'text': 'Ensures that the specified users exist and have at least the ensured permissions.\nThe MySQL users will be identified using Unix socket authentication. This authenticates the Unix user with the\nsame name only, and that without the need for a password.\nThis option will never delete existing users or remove permissions, especially not when the value of this\noption is changed. This means that users created and permissions assigned once through this option or\notherwise have to be removed manually.\n'}

*_Type_*
```
list of (submodule)
```


*_Default_*
```
[ ]
```


*_Example_*
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


## services.mysql.ensureUsers.*.ensurePermissions
{'_type': 'mdDoc', 'text': 'Permissions to ensure for the user, specified as attribute set.\nThe attribute names specify the database and tables to grant the permissions for,\nseparated by a dot. You may use wildcards here.\nThe attribute values specfiy the permissions to grant.\nYou may specify one or multiple comma-separated SQL privileges here.\nFor more information on how to specify the target\nand on which privileges exist, see the\n[GRANT syntax](https://mariadb.com/kb/en/library/grant/).\nThe attributes are used as `GRANT ${attrName} ON ${attrValue}`.\n'}

*_Type_*
```
attribute set of string
```


*_Default_*
```
{ }
```


*_Example_*
```
{
  "database.*" = "ALL PRIVILEGES";
  "*.*" = "SELECT, LOCK TABLES";
}

```


## services.mysql.ensureUsers.*.name
{'_type': 'mdDoc', 'text': 'Name of the user to ensure.\n'}

*_Type_*
```
string
```






## services.mysql.ensureUsers.*.password
{'_type': 'mdDoc', 'text': 'Password of the user to ensure.\n'}

*_Type_*
```
null or string
```


*_Default_*
```
null
```




## services.mysql.initialDatabases
List of database names and their initial schemas that should be used to create databases on the first startup
of MySQL. The schema attribute is optional: If not specified, an empty database is created.


*_Type_*
```
list of (submodule)
```


*_Default_*
```
[ ]
```


*_Example_*
```
[
  { name = "foodatabase"; schema = ./foodatabase.sql; }
  { name = "bardatabase"; }
]

```


## services.mysql.initialDatabases.*.name
The name of the database to create.


*_Type_*
```
string
```






## services.mysql.initialDatabases.*.schema
The initial schema of the database; if null (the default),
an empty database is created.


*_Type_*
```
null or path
```


*_Default_*
```
null
```




## services.mysql.package
Which package of mysql to use

*_Type_*
```
package
```


*_Default_*
```
pkgs.mysql80
```




## services.mysql.settings
MySQL configuration


*_Type_*
```
attribute set of attribute set of (INI atom (null, bool, int, float or string) or a list of them for duplicate keys)
```


*_Default_*
```
{ }
```


*_Example_*
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


## services.postgres.createDatabase
Create a database named like current user on startup. Only applies when initialDatabases is an empty list.


*_Type_*
```
boolean
```


*_Default_*
```
true
```




## services.postgres.enable
Whether to enable Add postgreSQL process script.
.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## services.postgres.initdbArgs
Additional arguments passed to `initdb` during data dir
initialisation.


*_Type_*
```
list of strings concatenated with "\n"
```


*_Default_*
```
[
  "--locale=C"
  "--encoding=UTF8"
]
```


*_Example_*
```
[
  "--data-checksums"
  "--allow-group-access"
]
```


## services.postgres.initialDatabases
List of database names and their initial schemas that should be used to create databases on the first startup
of Postgres. The schema attribute is optional: If not specified, an empty database is created.


*_Type_*
```
list of (submodule)
```


*_Default_*
```
[ ]
```


*_Example_*
```
[
  {
    name = "foodatabase";
    schema = ./foodatabase.sql;
  }
  { name = "bardatabase"; }
]

```


## services.postgres.initialDatabases.*.name
The name of the database to create.


*_Type_*
```
string
```






## services.postgres.initialDatabases.*.schema
The initial schema of the database; if null (the default),
an empty database is created.


*_Type_*
```
null or path
```


*_Default_*
```
null
```




## services.postgres.initialScript
Initial SQL commands to run during database initialization. This can be multiple
SQL expressions separated by a semi-colon.


*_Type_*
```
null or string
```


*_Default_*
```
null
```


*_Example_*
```
CREATE USER postgres SUPERUSER;
CREATE USER bar;

```


## services.postgres.listen_addresses
Listen address

*_Type_*
```
string
```


*_Default_*
```
""
```


*_Example_*
```
"127.0.0.1"
```


## services.postgres.package
Which version of postgres to use

*_Type_*
```
package
```


*_Default_*
```
pkgs.postgresql
```


*_Example_*
```
# see https://github.com/NixOS/nixpkgs/blob/master/pkgs/servers/sql/postgresql/packages.nix for full list
pkgs.postgresql_13.withPackages (p: [ p.pg_cron p.timescaledb p.pg_partman ]);

```


## services.postgres.port
The TCP port to accept connections.


*_Type_*
```
16 bit unsigned integer; between 0 and 65535 (both inclusive)
```


*_Default_*
```
5432
```




## services.postgres.settings
{'_type': 'mdDoc', 'text': 'PostgreSQL configuration. Refer to\n<https://www.postgresql.org/docs/11/config-setting.html#CONFIG-SETTING-CONFIGURATION-FILE>\nfor an overview of `postgresql.conf`.\n::: {.note}\nString values will automatically be enclosed in single quotes. Single quotes will be\nescaped with two single quotes as described by the upstream documentation linked above.\n:::\n'}

*_Type_*
```
attribute set of (boolean or floating point number or signed integer or string)
```


*_Default_*
```
{ }
```


*_Example_*
```
{
  log_connections = true;
  log_statement = "all";
  logging_collector = true
  log_disconnections = true
  log_destination = lib.mkForce "syslog";
}

```


## services.rabbitmq.configItems
Configuration options in RabbitMQ's new config file format,
which is a simple key-value format that can not express nested
data structures. This is known as the <literal>rabbitmq.conf</literal> file,
although outside NixOS that filename may have Erlang syntax, particularly
prior to RabbitMQ 3.7.0.
If you do need to express nested data structures, you can use
<literal>config</literal> option. Configuration from <literal>config</literal>
will be merged into these options by RabbitMQ at runtime to
form the final configuration.
See https://www.rabbitmq.com/configure.html#config-items
For the distinct formats, see https://www.rabbitmq.com/configure.html#config-file-formats


*_Type_*
```
attribute set of string
```


*_Default_*
```
{ }
```


*_Example_*
```
{
  "auth_backends.1.authn" = "rabbit_auth_backend_ldap";
  "auth_backends.1.authz" = "rabbit_auth_backend_internal";
}

```


## services.rabbitmq.cookie
Erlang cookie is a string of arbitrary length which must
be the same for several nodes to be allowed to communicate.
Leave empty to generate automatically.


*_Type_*
```
string
```


*_Default_*
```
""
```




## services.rabbitmq.enable
Whether to enable the RabbitMQ server, an Advanced Message
Queuing Protocol (AMQP) broker.


*_Type_*
```
boolean
```


*_Default_*
```
false
```




## services.rabbitmq.listenAddress
IP address on which RabbitMQ will listen for AMQP
connections.  Set to the empty string to listen on all
interfaces.  Note that RabbitMQ creates a user named
<literal>guest</literal> with password
<literal>guest</literal> by default, so you should delete
this user if you intend to allow external access.
Together with 'port' setting it's mostly an alias for
configItems."listeners.tcp.1" and it's left for backwards
compatibility with previous version of this module.


*_Type_*
```
string
```


*_Default_*
```
"127.0.0.1"
```


*_Example_*
```
""
```


## services.rabbitmq.managementPlugin.enable
Whether to enable the management plugin.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## services.rabbitmq.managementPlugin.port
On which port to run the management plugin


*_Type_*
```
16 bit unsigned integer; between 0 and 65535 (both inclusive)
```


*_Default_*
```
15672
```




## services.rabbitmq.package
Which rabbitmq package to use.


*_Type_*
```
package
```


*_Default_*
```
pkgs.rabbitmq-server
```




## services.rabbitmq.pluginDirs
The list of directories containing external plugins

*_Type_*
```
list of path
```


*_Default_*
```
[ ]
```




## services.rabbitmq.plugins
The names of plugins to enable

*_Type_*
```
list of string
```


*_Default_*
```
[ ]
```




## services.rabbitmq.port
Port on which RabbitMQ will listen for AMQP connections.


*_Type_*
```
16 bit unsigned integer; between 0 and 65535 (both inclusive)
```


*_Default_*
```
5672
```




## services.redis.bind
{'_type': 'mdDoc', 'text': 'The IP interface to bind to.\n`null` means "all interfaces".\n'}

*_Type_*
```
null or string
```


*_Default_*
```
"127.0.0.1"
```


*_Example_*
```
"127.0.0.1"
```


## services.redis.enable
Whether to enable redis process and expose utilities.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## services.redis.extraConfig
Additional text to be appended to <filename>redis.conf</filename>.

*_Type_*
```
strings concatenated with "\n"
```


*_Default_*
```
""
```




## services.redis.package
Which package of redis to use

*_Type_*
```
package
```


*_Default_*
```
pkgs.redis
```




## services.redis.port
The TCP port to accept connections.
If port 0 is specified Redis will not listen on a TCP socket.


*_Type_*
```
16 bit unsigned integer; between 0 and 65535 (both inclusive)
```


*_Default_*
```
6379
```




## services.wiremock.disableBanner
Whether to disable print banner logo


*_Type_*
```
boolean
```


*_Default_*
```
false
```




## services.wiremock.enable
Whether to enable wiremock.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## services.wiremock.mappings
The mappings to mock.
See the JSON examples on https://wiremock.org/docs/stubbing/ for more information.


*_Type_*
```
JSON value
```


*_Default_*
```
[ ]
```


*_Example_*
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


## services.wiremock.package
{'_type': 'mdDoc', 'text': 'Which package of wiremock to use.\n'}

*_Type_*
```
package
```


*_Default_*
```
pkgs.wiremock
```




## services.wiremock.port
The port number for the HTTP server to listen on.


*_Type_*
```
signed integer
```


*_Default_*
```
8080
```




## services.wiremock.verbose
Whether to log verbosely to stdout


*_Type_*
```
boolean
```


*_Default_*
```
false
```




## starship.config.enable
Whether to enable Starship config override.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## starship.config.path
The starship configuration file to use.

*_Type_*
```
path
```


*_Default_*
```
${config.env.DEVENV_ROOT}/starship.toml
```




## starship.enable
Whether to enable the Starship command prompt.

*_Type_*
```
boolean
```


*_Default_*
```
false
```


*_Example_*
```
true
```


## starship.package
The Starship package to use.

*_Type_*
```
package
```


*_Default_*
```
pkgs.starship
```




