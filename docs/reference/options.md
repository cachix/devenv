## _module.args
Additional arguments passed to each module in addition to ones
like <literal>lib</literal>, <literal>config</literal>,
and <literal>pkgs</literal>, <literal>modulesPath</literal>.

This option is also available to all submodules. Submodules do not
inherit args from their parent module, nor do they provide args to
their parent module or sibling submodules. The sole exception to
this is the argument <literal>name</literal> which is provided by
parent modules to a submodule and contains the attribute name
the submodule is bound to, or a unique generated name if it is
not bound to an attribute.

Some arguments are already passed by default, of which the
following <emphasis>cannot</emphasis> be changed with this option:

<itemizedlist>
<listitem><para><varname>lib</varname>: The nixpkgs library.

</para></listitem>
<listitem><para><varname>config</varname>: The results of all options after merging the values from all modules together.

</para></listitem>
<listitem><para><varname>options</varname>: The options declared in all modules.

</para></listitem>
<listitem><para><varname>specialArgs</varname>: The <literal>specialArgs</literal> argument passed to <literal>evalModules</literal>.

</para></listitem>
<listitem><para>All attributes of <varname>specialArgs</varname>

Whereas option values can generally depend on other option values
thanks to laziness, this does not apply to <literal>imports</literal>, which
must be computed statically before anything else.

For this reason, callers of the module system can provide <literal>specialArgs</literal>
which are available during import resolution.

For NixOS, <literal>specialArgs</literal> includes
<varname>modulesPath</varname>, which allows you to import
extra modules from the nixpkgs package tree without having to
somehow make the module aware of the location of the
<literal>nixpkgs</literal> or NixOS directories.

<programlisting>
{ modulesPath, ... }: {
  imports = [
    (modulesPath + "/profiles/minimal.nix")
  ];
}
</programlisting></para></listitem>

</itemizedlist>For NixOS, the default value for this option includes at least this argument:

<itemizedlist>
<listitem><para><varname>pkgs</varname>: The nixpkgs package set according to
the <option>nixpkgs.pkgs</option> option.</para></listitem>

</itemizedlist>


*_Type_*:
lazy attribute set of raw value






## enterShell
TODO

*_Type_*:
strings concatenated with "\n"


*_Default_*
```
""
```




## env
TODO

*_Type_*:
attribute set


*_Default_*
```
{}
```




## packages
TODO

*_Type_*:
list of package


*_Default_*
```
[]
```




## postgres.createDatabase
Create a database named like current user on startup.


*_Type_*:
boolean


*_Default_*
```
true
```




## postgres.enable
Whether to enable Add postgresql process and expose utilities..

*_Type_*:
boolean


*_Default_*
```
false
```


*_Example_*
```
true
```


## postgres.initdbArgs
Additional arguments passed to `initdb` during data dir
initialisation.


*_Type_*:
list of strings concatenated with "\n"


*_Default_*
```
["--no-locale"]
```


*_Example_*
```
["--data-checksums","--allow-group-access"]
```


## postgres.package
Which version of postgres to use

*_Type_*:
package


*_Default_*
```
"pkgs.postgresql"
```




## pre-commit
Integration of https://github.com/cachix/pre-commit-hooks.nix

*_Type_*:
submodule


*_Default_*
```
{}
```




## pre-commit.default_stages
A configuration wide option for the stages property.
Installs hooks to the defined stages.
See https://pre-commit.com/#confining-hooks-to-run-at-certain-stages


*_Type_*:
list of string


*_Default_*
```
["commit"]
```




## pre-commit.excludes
Exclude files that were matched by these patterns.


*_Type_*:
list of string


*_Default_*
```
[]
```




## pre-commit.hooks
The hook definitions.


*_Type_*:
attribute set of (submodule)


*_Default_*
```
{}
```




## pre-commit.hooks.\<name\>.description
Description of the hook. used for metadata purposes only.


*_Type_*:
string


*_Default_*
```
""
```




## pre-commit.hooks.\<name\>.enable
Whether to enable this pre-commit hook.

*_Type_*:
boolean


*_Default_*
```
false
```




## pre-commit.hooks.\<name\>.entry
The entry point - the executable to run. entry can also contain arguments that will not be overridden such as entry: autopep8 -i.


*_Type_*:
string






## pre-commit.hooks.\<name\>.excludes
Exclude files that were matched by these patterns.


*_Type_*:
list of string


*_Default_*
```
[]
```




## pre-commit.hooks.\<name\>.files
The pattern of files to run on.


*_Type_*:
string


*_Default_*
```
""
```




## pre-commit.hooks.\<name\>.language
The language of the hook - tells pre-commit how to install the hook.


*_Type_*:
string


*_Default_*
```
"system"
```




## pre-commit.hooks.\<name\>.name
The name of the hook - shown during hook execution.


*_Type_*:
string


*_Default_*
```
{"_type":"literalExpression","text":"internal name, same as id"}
```




## pre-commit.hooks.\<name\>.pass_filenames
Whether to pass filenames as arguments to the entry point.

*_Type_*:
boolean


*_Default_*
```
true
```




## pre-commit.hooks.\<name\>.raw
Raw fields of a pre-commit hook. This is mostly for internal use but
exposed in case you need to work around something.

Default: taken from the other hook options.


*_Type_*:
attribute set of unspecified value






## pre-commit.hooks.\<name\>.stages
Confines the hook to run at a particular stage.

*_Type_*:
list of string


*_Default_*
```
{"_type":"literalExpression","text":"default_stages"}
```




## pre-commit.hooks.\<name\>.types
List of file types to run on. See Filtering files with types (https://pre-commit.com/#plugins).


*_Type_*:
list of string


*_Default_*
```
["file"]
```




## pre-commit.hooks.\<name\>.types_or
List of file types to run on, where only a single type needs to match.


*_Type_*:
list of string


*_Default_*
```
[]
```




## pre-commit.installationScript
A bash snippet that installs nix-pre-commit in the current directory


*_Type_*:
string






## pre-commit.package
The pre-commit package to use.


*_Type_*:
package


*_Default_*
```
{"_type":"literalExpression","text":"pkgs.pre-commit\n"}
```




## pre-commit.rootSrc
The source of the project to be checked.

This is used in the derivation that performs the check.


*_Type_*:
path


*_Default_*
```
{"_type":"literalExpression","text":"gitignoreSource config.src"}
```




## pre-commit.run
A derivation that tests whether the pre-commit hooks run cleanly on
the entire project.


*_Type_*:
package


*_Default_*
```
{"_type":"derivation","name":"pre-commit-run"}
```




## pre-commit.settings.alejandra.exclude
Files or directories to exclude from formatting

*_Type_*:
list of string


*_Default_*
```
[]
```


*_Example_*
```
["flake.nix","./templates"]
```


## pre-commit.settings.deadnix.fix
Remove unused code and write to source file

*_Type_*:
boolean


*_Default_*
```
false
```




## pre-commit.settings.deadnix.noLambdaArg
Don't check lambda parameter arguments

*_Type_*:
boolean


*_Default_*
```
false
```




## pre-commit.settings.deadnix.noLambdaPatternNames
Don't check lambda pattern names (don't break nixpkgs callPackage)

*_Type_*:
boolean


*_Default_*
```
false
```




## pre-commit.settings.deadnix.noUnderscore
Don't check any bindings that start with a _

*_Type_*:
boolean


*_Default_*
```
false
```




## pre-commit.settings.deadnix.quiet
Don't print dead code report

*_Type_*:
boolean


*_Default_*
```
false
```




## pre-commit.settings.eslint.binPath
Eslint binary path. E.g. if you want to use the eslint in node_modules, use ./node_modules/.bin/eslint

*_Type_*:
path


*_Default_*
```
"/nix/store/14gw2smdm9gdh1cf9cc9bgmmjwry8pqh-eslint-8.26.0/bin/eslint"
```




## pre-commit.settings.eslint.extensions
The pattern of files to run on, see https://pre-commit.com/#hooks-files

*_Type_*:
string


*_Default_*
```
"\\.js$"
```




## pre-commit.settings.hpack.silent
Should generation should be silent

*_Type_*:
boolean


*_Default_*
```
false
```




## pre-commit.settings.nix-linter.checks
Available checks (See `nix-linter --help-for [CHECK]` for more details)

*_Type_*:
list of string


*_Default_*
```
[]
```




## pre-commit.settings.ormolu.cabalDefaultExtensions
Use default-extensions from .cabal files

*_Type_*:
boolean


*_Default_*
```
false
```




## pre-commit.settings.ormolu.defaultExtensions
Haskell language extensions to enable

*_Type_*:
list of string


*_Default_*
```
[]
```




## pre-commit.settings.prettier.binPath
Prettier binary path. E.g. if you want to use the prettier in node_modules, use ./node_modules/.bin/prettier

*_Type_*:
path


*_Default_*
```
"/nix/store/69kzc64shbq8mrafkldiy0bnj81kp246-prettier-2.7.1/bin/prettier"
```




## pre-commit.settings.revive.configPath
path to the configuration TOML file

*_Type_*:
string


*_Default_*
```
""
```




## pre-commit.settings.statix.format
Error Output format

*_Type_*:
one of "stderr", "errfmt", "json"


*_Default_*
```
"errfmt"
```




## pre-commit.settings.statix.ignore
Globs of file patterns to skip

*_Type_*:
list of string


*_Default_*
```
[]
```


*_Example_*
```
["flake.nix","_*"]
```


## pre-commit.src
Root of the project. By default this will be filtered with the gitignoreSource
function later, unless rootSrc is specified.


*_Type_*:
path






## pre-commit.tools
Tool set from which nix-pre-commit will pick binaries.

nix-pre-commit comes with its own set of packages for this purpose.


*_Type_*:
lazy attribute set of package


*_Default_*
```
{"_type":"literalExpression","text":"pre-commit-hooks.nix-pkgs.callPackage tools-dot-nix { inherit (pkgs) system; }"}
```




## processes
TODO

*_Type_*:
attribute set of (submodule)


*_Default_*
```
{}
```




## processes.\<name\>.exec
TODO

*_Type_*:
string






## scripts
TODO

*_Type_*:
attribute set of (submodule)


*_Default_*
```
{}
```




## scripts.\<name\>.exec
TODO

*_Type_*:
string






