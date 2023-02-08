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
"0.5.1"
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
Integrate difftastic into git: https://difftastic.wilfred.me.uk/.

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
Which package of Elixir to use.

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
Which package of Erlang to use.

*_Type_*
```
package
```


*_Default_*
```
pkgs.erlang
```




## languages.gawk.enable
Whether to enable tools for GNU Awk development.

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
The Gradle package to use.
The Gradle package by default inherits the JDK from `languages.java.jdk.package`.


*_Type_*
```
package
```






## languages.java.jdk.package
The JDK package to use.
This will also become available as `JAVA_HOME`.


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
The Maven package to use.
The Maven package by default inherits the JDK from `languages.java.jdk.package`.


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
Whether to enable tools for Nim development.

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
The Nim package to use.

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


## languages.ocaml.packages
The package set of OCaml to use

*_Type_*
```
attribute set
```


*_Default_*
```
pkgs.ocaml-ng.ocamlPackages_4_12
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


## languages.php.extensions
PHP extensions to enable.


*_Type_*
```
list of string
```


*_Default_*
```
[ ]
```




## languages.php.fpm.extraConfig
Extra configuration that should be put in the global section of
the PHP-FPM configuration file. Do not specify the options
`error_log` or `daemonize` here, since they are generated by
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
Options appended to the PHP configuration file `php.ini`.


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
See the documentation on `php-fpm.conf` for
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
Options appended to the PHP configuration file `php.ini` used for this PHP-FPM pool.


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
<https://www.php.net/manual/en/install.fpm.configuration.php">
the manual for details. Note that settings names must be
enclosed in quotes (e.g. `"pm.max_children"` instead of
`pm.max_children`).


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
Path to the Unix socket file on which to accept FastCGI requests.

This option is read-only and managed by NixOS.


*_Type_*
```
string
```




*_Example_*
```
"/.devenv/state/php-fpm/<name>.sock"
```


## languages.php.fpm.settings
PHP-FPM global directives. 

Refer to the "List of global php-fpm.conf directives" section of
<https://www.php.net/manual/en/install.fpm.configuration.php>
for details. 

Note that settings names must be enclosed in
quotes (e.g. `"pm.max_children"` instead of `pm.max_children`). 

You need not specify the options `error_log` or `daemonize` here, since
they are already set.


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




## languages.php.ini
PHP.ini directives. Refer to the "List of php.ini directives" of PHP's


*_Type_*
```
null or strings concatenated with "\n"
```


*_Default_*
```
""
```




## languages.php.package
Allows you to [override the default used package](https://nixos.org/manual/nixpkgs/stable/#ssec-php-user-guide)
to adjust the settings or add more extensions. You can find the
extensions using `devenv search 'php extensions'`


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


## languages.php.packages
Attribute set of packages including composer

*_Type_*
```
submodule
```


*_Default_*
```
pkgs
```




## languages.php.packages.composer
composer package

*_Type_*
```
null or package
```


*_Default_*
```
pkgs.phpPackages.composer
```




## languages.php.version
The PHP version to use.

*_Type_*
```
string
```


*_Default_*
```
""
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
The Poetry package to use.

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




## languages.raku.enable
Whether to enable Enable tools for Raku development..

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
Attribute set of packages including rustc and Cargo.

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
Set to stable, beta, or latest.

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




## languages.swift.enable
Whether to enable tools for Swift development.

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


## languages.swift.package
The Swift package to use.


*_Type_*
```
package
```


*_Default_*
```
"pkgs.swift"
```




## languages.terraform.enable
Whether to enable tools for Terraform development.

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
The Terraform package to use.

*_Type_*
```
package
```


*_Default_*
```
pkgs.terraform
```




## languages.texlive.base
TeX Live package set to use

*_Type_*
```
unspecified value
```


*_Default_*
```
pkgs.texlive
```




## languages.texlive.enable
Whether to enable TeX Live.

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


## languages.texlive.packages
Packages available to TeX Live

*_Type_*
```
non-empty (list of Concatenated string)
```


*_Default_*
```
[
  "collection-basic"
]
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
Whether to enable tools for V development.

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
The V package to use.

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
Which package of Zig to use.

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
A configuration wide option for the stages property.
Installs hooks to the defined stages.
See [https://pre-commit.com/#confining-hooks-to-run-at-certain-stages](https://pre-commit.com/#confining-hooks-to-run-at-certain-stages).


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
Exclude files that were matched by these patterns.


*_Type_*
```
list of string
```


*_Default_*
```
[ ]
```




## pre-commit.hooks
The hook definitions.

Pre-defined hooks can be enabled by, for example:

```nix
hooks.nixpkgs-fmt.enable = true;
```

The pre-defined hooks are:

**`actionlint`**

Static checker for GitHub Actions workflow files.


**`alejandra`**

The Uncompromising Nix Code Formatter.


**`ansible-lint`**

Ansible linter.


**`autoflake`**

Remove unused imports and variables from Python code.


**`bats`**

Run bash unit tests.


**`black`**

The uncompromising Python code formatter.


**`cabal-fmt`**

Format Cabal files


**`cabal2nix`**

Run `cabal2nix` on all `*.cabal` files to generate corresponding `default.nix` files.


**`cargo-check`**

Check the cargo package for errors.


**`chktex`**

LaTeX semantic checker


**`clang-format`**

Format your code using `clang-format`.


**`clippy`**

Lint Rust code.


**`commitizen`**

Check whether the current commit message follows commiting rules.



**`deadnix`**

Scan Nix files for dead code (unused variable bindings).


**`dhall-format`**

Dhall code formatter.


**`editorconfig-checker`**

Verify that the files are in harmony with the `.editorconfig`.


**`elm-format`**

Format Elm files.


**`elm-review`**

Analyzes Elm projects, to help find mistakes before your users find them.


**`elm-test`**

Run unit tests and fuzz tests for Elm code.


**`eslint`**

Find and fix problems in your JavaScript code.


**`flake8`**

Check the style and quality of Python files.


**`fourmolu`**

Haskell code prettifier.


**`gofmt`**

A tool that automatically formats Go source code


**`gotest`**

Run go tests


**`govet`**

Checks correctness of Go programs.


**`hadolint`**

Dockerfile linter, validate inline bash.


**`hindent`**

Haskell code prettifier.


**`hlint`**

HLint gives suggestions on how to improve your source code.


**`hpack`**

`hpack` converts package definitions in the hpack format (`package.yaml`) to Cabal files.


**`html-tidy`**

HTML linter.


**`hunspell`**

Spell checker and morphological analyzer.


**`isort`**

A Python utility / library to sort imports.


**`latexindent`**

Perl script to add indentation to LaTeX files.


**`luacheck`**

A tool for linting and static analysis of Lua code.


**`markdownlint`**

Style checker and linter for markdown files.


**`mdsh`**

Markdown shell pre-processor.


**`nixfmt`**

Nix code prettifier.


**`nixpkgs-fmt`**

Nix code prettifier.


**`ormolu`**

Haskell code prettifier.


**`php-cs-fixer`**

Lint PHP files.


**`phpcbf`**

Lint PHP files.


**`phpcs`**

Lint PHP files.


**`prettier`**

Opinionated multi-language code formatter.


**`purs-tidy`**

Format purescript files.


**`purty`**

Format purescript files.


**`pylint`**

Lint Python files.


**`revive`**

A linter for Go source code.


**`ruff`**

 An extremely fast Python linter, written in Rust.


**`rustfmt`**

Format Rust code.


**`shellcheck`**

Format shell files.


**`shfmt`**

Format shell files.


**`staticcheck`**

State of the art linter for the Go programming language


**`statix`**

Lints and suggestions for the Nix programming language.


**`stylish-haskell`**

A simple Haskell code prettifier


**`stylua`**

An Opinionated Lua Code Formatter.


**`terraform-format`**

Format terraform (`.tf`) files.


**`typos`**

Source code spell checker


**`yamllint`**

Yaml linter.




*_Type_*
```
attribute set of (submodule)
```


*_Default_*
```
{ }
```




## pre-commit.hooks.&lt;name&gt;.description
Description of the hook. used for metadata purposes only.


*_Type_*
```
string
```


*_Default_*
```
""
```




## pre-commit.hooks.&lt;name&gt;.enable
Whether to enable this pre-commit hook.

*_Type_*
```
boolean
```


*_Default_*
```
false
```




## pre-commit.hooks.&lt;name&gt;.entry
The entry point - the executable to run. {option}`entry` can also contain arguments that will not be overridden, such as `entry = "autopep8 -i";`.


*_Type_*
```
string
```






## pre-commit.hooks.&lt;name&gt;.excludes
Exclude files that were matched by these patterns.


*_Type_*
```
list of string
```


*_Default_*
```
[ ]
```




## pre-commit.hooks.&lt;name&gt;.files
The pattern of files to run on.


*_Type_*
```
string
```


*_Default_*
```
""
```




## pre-commit.hooks.&lt;name&gt;.language
The language of the hook - tells pre-commit how to install the hook.


*_Type_*
```
string
```


*_Default_*
```
"system"
```




## pre-commit.hooks.&lt;name&gt;.name
The name of the hook - shown during hook execution.


*_Type_*
```
string
```


*_Default_*
```
internal name, same as id
```




## pre-commit.hooks.&lt;name&gt;.pass_filenames
Whether to pass filenames as arguments to the entry point.


*_Type_*
```
boolean
```


*_Default_*
```
true
```




## pre-commit.hooks.&lt;name&gt;.raw
Raw fields of a pre-commit hook. This is mostly for internal use but
exposed in case you need to work around something.

Default: taken from the other hook options.


*_Type_*
```
attribute set of unspecified value
```






## pre-commit.hooks.&lt;name&gt;.stages
Confines the hook to run at a particular stage.


*_Type_*
```
list of string
```


*_Default_*
```
default_stages
```




## pre-commit.hooks.&lt;name&gt;.types
List of file types to run on. See [Filtering files with types](https://pre-commit.com/#plugins).


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
List of file types to run on, where only a single type needs to match.


*_Type_*
```
list of string
```


*_Default_*
```
[ ]
```




## pre-commit.installationScript
A bash snippet that installs nix-pre-commit-hooks in the current directory


*_Type_*
```
string
```






## pre-commit.package
The `pre-commit` package to use.


*_Type_*
```
package
```






## pre-commit.rootSrc
The source of the project to be checked.

This is used in the derivation that performs the check.

If you use the `flakeModule`, the default is `self.outPath`; the whole flake
sources.


*_Type_*
```
path
```






## pre-commit.run
A derivation that tests whether the pre-commit hooks run cleanly on
the entire project.


*_Type_*
```
package
```


*_Default_*
```
"<derivation>"
```




## pre-commit.settings.alejandra.exclude
Files or directories to exclude from formatting.

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
Path to autoflake binary.

*_Type_*
```
string
```


*_Default_*
```
"${pkgs.autoflake}/bin/autoflake"

```




## pre-commit.settings.autoflake.flags
Flags passed to autoflake.

*_Type_*
```
string
```


*_Default_*
```
"--in-place --expand-star-imports --remove-duplicate-keys --remove-unused-variables"
```




## pre-commit.settings.clippy.denyWarnings
Fail when warnings are present

*_Type_*
```
boolean
```


*_Default_*
```
false
```




## pre-commit.settings.deadnix.edit
Remove unused code and write to source file.

*_Type_*
```
boolean
```


*_Default_*
```
false
```




## pre-commit.settings.deadnix.noLambdaArg
Don't check lambda parameter arguments.

*_Type_*
```
boolean
```


*_Default_*
```
false
```




## pre-commit.settings.deadnix.noLambdaPatternNames
Don't check lambda pattern names (don't break nixpkgs `callPackage`).

*_Type_*
```
boolean
```


*_Default_*
```
false
```




## pre-commit.settings.deadnix.noUnderscore
Don't check any bindings that start with a `_`.

*_Type_*
```
boolean
```


*_Default_*
```
false
```




## pre-commit.settings.deadnix.quiet
Don't print a dead code report.

*_Type_*
```
boolean
```


*_Default_*
```
false
```




## pre-commit.settings.eslint.binPath
`eslint` binary path. E.g. if you want to use the `eslint` in `node_modules`, use `./node_modules/.bin/eslint`.

*_Type_*
```
path
```


*_Default_*
```
${tools.eslint}/bin/eslint
```




## pre-commit.settings.eslint.extensions
The pattern of files to run on, see [https://pre-commit.com/#hooks-files](https://pre-commit.com/#hooks-files).

*_Type_*
```
string
```


*_Default_*
```
"\\.js$"
```




## pre-commit.settings.flake8.binPath
flake8 binary path. Should be used to specify flake8 binary from your Nix-managed Python environment.

*_Type_*
```
string
```


*_Default_*
```
"${pkgs.python39Packages.flake8}/bin/flake8"

```




## pre-commit.settings.flake8.format
Output format.

*_Type_*
```
string
```


*_Default_*
```
"default"
```




## pre-commit.settings.hpack.silent
Whether generation should be silent.

*_Type_*
```
boolean
```


*_Default_*
```
false
```




## pre-commit.settings.markdownlint.config
See https://github.com/DavidAnson/markdownlint/blob/main/schema/.markdownlint.jsonc

*_Type_*
```
attribute set
```


*_Default_*
```
{ }
```




## pre-commit.settings.nixfmt.width
Line width.

*_Type_*
```
null or signed integer
```


*_Default_*
```
null
```




## pre-commit.settings.ormolu.cabalDefaultExtensions
Use `default-extensions` from `.cabal` files.

*_Type_*
```
boolean
```


*_Default_*
```
false
```




## pre-commit.settings.ormolu.defaultExtensions
Haskell language extensions to enable.

*_Type_*
```
list of string
```


*_Default_*
```
[ ]
```




## pre-commit.settings.php-cs-fixer.binPath
PHP-CS-Fixer binary path.

*_Type_*
```
string
```


*_Default_*
```
"${pkgs.php81Packages.php-cs-fixer}/bin/php-cs-fixer"

```




## pre-commit.settings.phpcbf.binPath
PHP_CodeSniffer binary path.

*_Type_*
```
string
```


*_Default_*
```
"${pkgs.php80Packages.phpcbf}/bin/phpcbf"

```




## pre-commit.settings.phpcs.binPath
PHP_CodeSniffer binary path.

*_Type_*
```
string
```


*_Default_*
```
"${pkgs.php80Packages.phpcs}/bin/phpcs"

```




## pre-commit.settings.prettier.binPath
`prettier` binary path. E.g. if you want to use the `prettier` in `node_modules`, use `./node_modules/.bin/prettier`.

*_Type_*
```
path
```


*_Default_*
```
"${tools.prettier}/bin/prettier"

```




## pre-commit.settings.prettier.output
Output format.

*_Type_*
```
null or one of "check", "list-different"
```


*_Default_*
```
"list-different"
```




## pre-commit.settings.prettier.write
Whether to edit files inplace.

*_Type_*
```
boolean
```


*_Default_*
```
true
```




## pre-commit.settings.pylint.binPath
Pylint binary path. Should be used to specify Pylint binary from your Nix-managed Python environment.

*_Type_*
```
string
```


*_Default_*
```
"${pkgs.python39Packages.pylint}/bin/pylint"

```




## pre-commit.settings.pylint.reports
Whether to display a full report.

*_Type_*
```
boolean
```


*_Default_*
```
false
```




## pre-commit.settings.pylint.score
Whether to activate the evaluation score.

*_Type_*
```
boolean
```


*_Default_*
```
true
```




## pre-commit.settings.revive.configPath
Path to the configuration TOML file.

*_Type_*
```
string
```


*_Default_*
```
""
```




## pre-commit.settings.rust.cargoManifestPath
Path to Cargo.toml

*_Type_*
```
null or string
```


*_Default_*
```
null
```




## pre-commit.settings.statix.format
Error Output format.

*_Type_*
```
one of "stderr", "errfmt", "json"
```


*_Default_*
```
"errfmt"
```




## pre-commit.settings.statix.ignore
Globs of file patterns to skip.

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
Wheter to print a diff of what would change.

*_Type_*
```
boolean
```


*_Default_*
```
false
```




## pre-commit.settings.typos.format
Output format.

*_Type_*
```
one of "silent", "brief", "long", "json"
```


*_Default_*
```
"long"
```




## pre-commit.settings.typos.write
Whether to write fixes out.

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
Use the relaxed configuration

*_Type_*
```
boolean
```


*_Default_*
```
false
```




## pre-commit.src
Root of the project. By default this will be filtered with the `gitignoreSource`
function later, unless `rootSrc` is specified.

If you use the `flakeModule`, the default is `self.outPath`; the whole flake
sources.


*_Type_*
```
path
```






## pre-commit.tools
Tool set from which `nix-pre-commit-hooks` will pick binaries.

`nix-pre-commit-hooks` comes with its own set of packages for this purpose.


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
Bash code to execute when the script is run.

*_Type_*
```
string
```






## services.adminer.enable
Whether to enable Adminer process.

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
Listen address for the Adminer.

*_Type_*
```
string
```


*_Default_*
```
"127.0.0.1:8080"
```




## services.adminer.package
Which package of Adminer to use.

*_Type_*
```
package
```


*_Default_*
```
pkgs.adminer
```




## services.blackfire.client-id
Sets the client id used to authenticate with Blackfire.
You can find your personal client-id at <https://blackfire.io/my/settings/credentials>.


*_Type_*
```
string
```


*_Default_*
```
""
```




## services.blackfire.client-token
Sets the client token used to authenticate with Blackfire.
You can find your personal client-token at <https://blackfire.io/my/settings/credentials>.


*_Type_*
```
string
```


*_Default_*
```
""
```




## services.blackfire.enable
Whether to enable Blackfire profiler agent

It automatically installs Blackfire PHP extension.
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
Sets the server id used to authenticate with Blackfire.
You can find your personal server-id at <https://blackfire.io/my/settings/credentials>.


*_Type_*
```
string
```


*_Default_*
```
""
```




## services.blackfire.server-token
Sets the server token used to authenticate with Blackfire.
You can find your personal server-token at <https://blackfire.io/my/settings/credentials>.


*_Type_*
```
string
```


*_Default_*
```
""
```




## services.blackfire.socket
Sets the server socket path


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
See <https://caddyserver.com/docs/config-adapters> for the full list.


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
fine-graned configuration manually).


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
Caddy v2 supports multiple config formats via adapters (see [`services.caddy.adapter`](#servicescaddyconfig)).


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
See <https://caddyserver.com/docs/conventions#file-locations>.


*_Type_*
```
path
```


*_Default_*
```
"/.devenv/state/caddy"
```




## services.caddy.email
Email address (for Let's Encrypt certificate).

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
Use saved config, if any (and prefer over configuration passed with [`caddy.config`](#caddyconfig)).


*_Type_*
```
boolean
```


*_Default_*
```
false
```




## services.caddy.virtualHosts
Declarative vhost config.

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
These lines go into the vhost verbatim.


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
The IP interface to bind to.
`null` means "all interfaces".


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
Access key of 5 to 20 characters in length that clients use to access the server.
This overrides the access key that is generated by MinIO on first startup and stored inside the
`configDir` directory.


*_Type_*
```
string
```


*_Default_*
```
""
```




## services.minio.browser
Enable or disable access to web UI.

*_Type_*
```
boolean
```


*_Default_*
```
true
```




## services.minio.buckets
List of buckets to ensure exist on startup.


*_Type_*
```
list of string
```


*_Default_*
```
[ ]
```




## services.minio.consoleAddress
IP address and port of the web UI (console).

*_Type_*
```
string
```


*_Default_*
```
"127.0.0.1:9001"
```




## services.minio.enable
Whether to enable MinIO Object Storage.

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
IP address and port of the server.

*_Type_*
```
string
```


*_Default_*
```
"127.0.0.1:9000"
```




## services.minio.package
MinIO package to use.

*_Type_*
```
package
```


*_Default_*
```
pkgs.minio
```




## services.minio.region
The physical location of the server. By default it is set to us-east-1, which is same as AWS S3's and MinIO's default region.


*_Type_*
```
string
```


*_Default_*
```
"us-east-1"
```




## services.minio.secretKey
Specify the Secret key of 8 to 40 characters in length that clients use to access the server.
This overrides the secret key that is generated by MinIO on first startup and stored inside the
`configDir` directory.


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
Whether to enable MySQL process and expose utilities.

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
Ensures that the specified users exist and have at least the ensured permissions.
The MySQL users will be identified using Unix socket authentication. This authenticates the Unix user with the
same name only, and that without the need for a password.
This option will never delete existing users or remove permissions, especially not when the value of this
option is changed. This means that users created and permissions assigned once through this option or
otherwise have to be removed manually.


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
Permissions to ensure for the user, specified as attribute set.
The attribute names specify the database and tables to grant the permissions for,
separated by a dot. You may use wildcards here.
The attribute values specfiy the permissions to grant.
You may specify one or multiple comma-separated SQL privileges here.
For more information on how to specify the target
and on which privileges exist, see the
[GRANT syntax](https://mariadb.com/kb/en/library/grant/).
The attributes are used as `GRANT ${attrName} ON ${attrValue}`.


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
Name of the user to ensure.


*_Type_*
```
string
```






## services.mysql.ensureUsers.*.password
Password of the user to ensure.


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
Which package of MySQL to use

*_Type_*
```
package
```


*_Default_*
```
pkgs.mysql80
```




## services.mysql.settings
MySQL configuration.


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
Whether to enable Add PostgreSQL process.
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
Which version of PostgreSQL to use

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
PostgreSQL configuration. Refer to
<https://www.postgresql.org/docs/11/config-setting.html#CONFIG-SETTING-CONFIGURATION-FILE>
for an overview of `postgresql.conf`.

String values will automatically be enclosed in single quotes. Single quotes will be
escaped with two single quotes as described by the upstream documentation linked above.


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
data structures. This is known as the `rabbitmq.conf` file,
although outside NixOS that filename may have Erlang syntax, particularly
prior to RabbitMQ 3.7.0.
If you do need to express nested data structures, you can use
`config` option. Configuration from `config`
will be merged into these options by RabbitMQ at runtime to
form the final configuration.
See <https://www.rabbitmq.com/configure.html#config-items>
For the distinct formats, see <https://www.rabbitmq.com/configure.html#config-file-formats>


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
`guest` with password
`guest` by default, so you should delete
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
The IP interface to bind to.
`null` means "all interfaces".


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
Whether to enable Redis process and expose utilities.

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
Additional text to be appended to `redis.conf`.

*_Type_*
```
strings concatenated with "\n"
```


*_Default_*
```
""
```




## services.redis.package
Which package of Redis to use

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
If port 0 is specified Redis, will not listen on a TCP socket.


*_Type_*
```
16 bit unsigned integer; between 0 and 65535 (both inclusive)
```


*_Default_*
```
6379
```




## services.wiremock.disableBanner
Whether to disable print banner logo.


*_Type_*
```
boolean
```


*_Default_*
```
false
```




## services.wiremock.enable
Whether to enable WireMock.

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
See the JSON examples on <https://wiremock.org/docs/stubbing/> for more information.


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
Which package of WireMock to use.


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
Whether to log verbosely to stdout.


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
The Starship configuration file to use.

*_Type_*
```
path
```


*_Default_*
```
${config.env.DEVENV_ROOT}/starship.toml
```




## starship.enable
Whether to enable the Starship.rs command prompt.

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




