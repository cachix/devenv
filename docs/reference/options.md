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
The JDK package to use.
This will also become available as <literal>JAVA_HOME</literal>.


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
The maven package to use.
The maven package by default inherits the JDK from <literal>languages.java.jdk.package</literal>.


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
Allows to <link xlink:href="https://nixos.org/manual/nixpkgs/stable/#ssec-php-user-guide">override the default used package</link> to adjust the settings or add more extensions. You can find the extensions using <literal>devenv search 'php extensions'</literal>

<programlisting>

</programlisting>


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
A configuration wide option for the stages property.
Installs hooks to the defined stages.
See <link xlink:href="https://pre-commit.com/#confining-hooks-to-run-at-certain-stages"></link>.


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

<programlisting language="nix">
hooks.nixpkgs-fmt.enable = true;
</programlisting>The pre-defined hooks are:

<emphasis role="strong"><literal>actionlint</literal></emphasis>

Static checker for GitHub Actions workflow files.

<emphasis role="strong"><literal>alejandra</literal></emphasis>

The Uncompromising Nix Code Formatter.

<emphasis role="strong"><literal>ansible-lint</literal></emphasis>

Ansible linter.

<emphasis role="strong"><literal>black</literal></emphasis>

The uncompromising Python code formatter.

<emphasis role="strong"><literal>brittany</literal></emphasis>

Haskell source code formatter.

<emphasis role="strong"><literal>cabal-fmt</literal></emphasis>

Format Cabal files

<emphasis role="strong"><literal>cabal2nix</literal></emphasis>

Run <literal>cabal2nix</literal> on all <literal>*.cabal</literal> files to generate corresponding <literal>default.nix</literal> files.

<emphasis role="strong"><literal>cargo-check</literal></emphasis>

Check the cargo package for errors.

<emphasis role="strong"><literal>chktex</literal></emphasis>

LaTeX semantic checker

<emphasis role="strong"><literal>clang-format</literal></emphasis>

Format your code using <literal>clang-format</literal>.

<emphasis role="strong"><literal>clippy</literal></emphasis>

Lint Rust code.

<emphasis role="strong"><literal>commitizen</literal></emphasis>

Check whether the current commit message follows commiting rules.

<emphasis role="strong"><literal>deadnix</literal></emphasis>

Scan Nix files for dead code (unused variable bindings).

<emphasis role="strong"><literal>dhall-format</literal></emphasis>

Dhall code formatter.

<emphasis role="strong"><literal>editorconfig-checker</literal></emphasis>

Verify that the files are in harmony with the <literal>.editorconfig</literal>.

<emphasis role="strong"><literal>elm-format</literal></emphasis>

Format Elm files.

<emphasis role="strong"><literal>elm-review</literal></emphasis>

Analyzes Elm projects, to help find mistakes before your users find them.

<emphasis role="strong"><literal>elm-test</literal></emphasis>

Run unit tests and fuzz tests for Elm code.

<emphasis role="strong"><literal>eslint</literal></emphasis>

Find and fix problems in your JavaScript code.

<emphasis role="strong"><literal>flake8</literal></emphasis>

Check the style and quality of Python files.

<emphasis role="strong"><literal>fourmolu</literal></emphasis>

Haskell code prettifier.

<emphasis role="strong"><literal>gofmt</literal></emphasis>

A tool that automatically formats Go source code

<emphasis role="strong"><literal>gotest</literal></emphasis>

Run go tests

<emphasis role="strong"><literal>govet</literal></emphasis>

Checks correctness of Go programs.

<emphasis role="strong"><literal>hadolint</literal></emphasis>

Dockerfile linter, validate inline bash.

<emphasis role="strong"><literal>hindent</literal></emphasis>

Haskell code prettifier.

<emphasis role="strong"><literal>hlint</literal></emphasis>

HLint gives suggestions on how to improve your source code.

<emphasis role="strong"><literal>hpack</literal></emphasis>

<literal>hpack</literal> converts package definitions in the hpack format (<literal>package.yaml</literal>) to Cabal files.

<emphasis role="strong"><literal>html-tidy</literal></emphasis>

HTML linter.

<emphasis role="strong"><literal>hunspell</literal></emphasis>

Spell checker and morphological analyzer.

<emphasis role="strong"><literal>isort</literal></emphasis>

A Python utility / library to sort imports.

<emphasis role="strong"><literal>latexindent</literal></emphasis>

Perl script to add indentation to LaTeX files.

<emphasis role="strong"><literal>luacheck</literal></emphasis>

A tool for linting and static analysis of Lua code.

<emphasis role="strong"><literal>markdownlint</literal></emphasis>

Style checker and linter for markdown files.

<emphasis role="strong"><literal>mdsh</literal></emphasis>

Markdown shell pre-processor.

<emphasis role="strong"><literal>nix-linter</literal></emphasis>

Linter for the Nix expression language.

<emphasis role="strong"><literal>nixfmt</literal></emphasis>

Nix code prettifier.

<emphasis role="strong"><literal>nixpkgs-fmt</literal></emphasis>

Nix code prettifier.

<emphasis role="strong"><literal>ormolu</literal></emphasis>

Haskell code prettifier.

<emphasis role="strong"><literal>php-cs-fixer</literal></emphasis>

Lint PHP files.

<emphasis role="strong"><literal>phpcbf</literal></emphasis>

Lint PHP files.

<emphasis role="strong"><literal>phpcs</literal></emphasis>

Lint PHP files.

<emphasis role="strong"><literal>prettier</literal></emphasis>

Opinionated multi-language code formatter.

<emphasis role="strong"><literal>purs-tidy</literal></emphasis>

Format purescript files.

<emphasis role="strong"><literal>purty</literal></emphasis>

Format purescript files.

<emphasis role="strong"><literal>pylint</literal></emphasis>

Lint Python files.

<emphasis role="strong"><literal>revive</literal></emphasis>

A linter for Go source code.

<emphasis role="strong"><literal>rustfmt</literal></emphasis>

Format Rust code.

<emphasis role="strong"><literal>shellcheck</literal></emphasis>

Format shell files.

<emphasis role="strong"><literal>shfmt</literal></emphasis>

Format shell files.

<emphasis role="strong"><literal>staticcheck</literal></emphasis>

State of the art linter for the Go programming language

<emphasis role="strong"><literal>statix</literal></emphasis>

Lints and suggestions for the Nix programming language.

<emphasis role="strong"><literal>stylish-haskell</literal></emphasis>

A simple Haskell code prettifier

<emphasis role="strong"><literal>stylua</literal></emphasis>

An Opinionated Lua Code Formatter.

<emphasis role="strong"><literal>terraform-format</literal></emphasis>

Format terraform (<literal>.tf</literal>) files.

<emphasis role="strong"><literal>typos</literal></emphasis>

Source code spell checker

<emphasis role="strong"><literal>yamllint</literal></emphasis>

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
The entry point - the executable to run. <option>entry</option> can also contain arguments that will not be overridden, such as <literal>entry = "autopep8 -i";</literal>.


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
List of file types to run on. See <link xlink:href="https://pre-commit.com/#plugins">Filtering files with types</link>.


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
The <literal>pre-commit</literal> package to use.


*_Type_*
```
package
```






## pre-commit.rootSrc
The source of the project to be checked.

This is used in the derivation that performs the check.

If you use the <literal>flakeModule</literal>, the default is <literal>self.outPath</literal>; the whole flake
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
Don't check lambda pattern names (don't break nixpkgs <literal>callPackage</literal>).

*_Type_*
```
boolean
```


*_Default_*
```
false
```




## pre-commit.settings.deadnix.noUnderscore
Don't check any bindings that start with a <literal>_</literal>.

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
<literal>eslint</literal> binary path. E.g. if you want to use the <literal>eslint</literal> in <literal>node_modules</literal>, use <literal>./node_modules/.bin/eslint</literal>.

*_Type_*
```
path
```


*_Default_*
```
${tools.eslint}/bin/eslint
```




## pre-commit.settings.eslint.extensions
The pattern of files to run on, see <link xlink:href="https://pre-commit.com/#hooks-files"></link>.

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
"${pkgs.python39Packages.pylint}/bin/flake8"

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




## pre-commit.settings.nix-linter.checks
Available checks. See <literal>nix-linter --help-for [CHECK]</literal> for more details.

*_Type_*
```
list of string
```


*_Default_*
```
[ ]
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
Use <literal>default-extensions</literal> from <literal>.cabal</literal> files.

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
<literal>prettier</literal> binary path. E.g. if you want to use the <literal>prettier</literal> in <literal>node_modules</literal>, use <literal>./node_modules/.bin/prettier</literal>.

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
Root of the project. By default this will be filtered with the <literal>gitignoreSource</literal>
function later, unless <literal>rootSrc</literal> is specified.

If you use the <literal>flakeModule</literal>, the default is <literal>self.outPath</literal>; the whole flake
sources.


*_Type_*
```
path
```






## pre-commit.tools
Tool set from which <literal>nix-pre-commit-hooks</literal> will pick binaries.

<literal>nix-pre-commit-hooks</literal> comes with its own set of packages for this purpose.


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
Sets the client id used to authenticate with Blackfire
You can find your personal client-id at https://blackfire.io/my/settings/credentials


*_Type_*
```
string
```


*_Default_*
```
""
```




## services.blackfire.client-token
Sets the client token used to authenticate with Blackfire
You can find your personal client-token at https://blackfire.io/my/settings/credentials


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

For PHP you need to install and configure the Blackfire PHP extension.

<programlisting language="nix">
languages.php.package = pkgs.php.buildEnv {
  extensions = { all, enabled }: with all; enabled ++ [ (blackfire// { extensionName = "blackfire"; }) ];
  extraConfig = ''
    memory_limit = 256M
    blackfire.agent_socket = "tcp://127.0.0.1:8307";
  '';
};
</programlisting>.

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
Sets the server id used to authenticate with Blackfire
You can find your personal server-id at https://blackfire.io/my/settings/credentials


*_Type_*
```
string
```


*_Default_*
```
""
```




## services.blackfire.server-token
Sets the server token used to authenticate with Blackfire
You can find your personal server-token at https://blackfire.io/my/settings/credentials


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
The IP interface to bind to.
<literal>null</literal> means "all interfaces".


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
This overrides the access key that is generated by minio on first startup and stored inside the
<literal>configDir</literal> directory.


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
Whether to enable Minio Object Storage.

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
Minio package to use.

*_Type_*
```
package
```


*_Default_*
```
pkgs.minio
```




## services.minio.region
The physical location of the server. By default it is set to us-east-1, which is same as AWS S3's and Minio's default region.


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
This overrides the secret key that is generated by minio on first startup and stored inside the
<literal>configDir</literal> directory.


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
<link xlink:href="https://mariadb.com/kb/en/library/grant/">GRANT syntax</link>.
The attributes are used as <literal>GRANT ${attrName} ON ${attrValue}</literal>.


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
PostgreSQL configuration. Refer to
<link xlink:href="https://www.postgresql.org/docs/11/config-setting.html#CONFIG-SETTING-CONFIGURATION-FILE"></link>
for an overview of <literal>postgresql.conf</literal>.
::: {.note}
String values will automatically be enclosed in single quotes. Single quotes will be
escaped with two single quotes as described by the upstream documentation linked above.
:::


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
The IP interface to bind to.
<literal>null</literal> means "all interfaces".


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
Which package of wiremock to use.


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




