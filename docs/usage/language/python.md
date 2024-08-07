  # Python
  


## languages\.python\.enable



Whether to enable tools for Python development\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## languages\.python\.package



The Python package to use\.



*Type:*
package



*Default:*
` pkgs.python3 `



## languages\.python\.directory

The Python project’s root directory\. Defaults to the root of the devenv project\.
Can be an absolute path or one relative to the root of the devenv project\.



*Type:*
string



*Default:*
` config.devenv.root `



*Example:*
` "./directory" `



## languages\.python\.libraries



Additional libraries to make available to the Python interpreter\.

This is useful when you want to use Python wheels that depend on native libraries\.



*Type:*
list of path



*Default:*

```
[ "${config.devenv.dotfile}/profile" ]

```



## languages\.python\.manylinux\.enable



Whether to install manylinux2014 libraries\.

Enabled by default on linux;

This is useful when you want to use Python wheels that depend on manylinux2014 libraries\.



*Type:*
boolean



*Default:*
` true `



## languages\.python\.poetry\.enable



Whether to enable poetry\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## languages\.python\.poetry\.package



The Poetry package to use\.



*Type:*
package



*Default:*
` pkgs.poetry `



## languages\.python\.poetry\.activate\.enable



Whether to activate the poetry virtual environment automatically\.



*Type:*
boolean



*Default:*
` false `



## languages\.python\.poetry\.install\.enable



Whether to enable poetry install during devenv initialisation\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## languages\.python\.poetry\.install\.allExtras



Whether to install all extras\. See ` --all-extras `\.



*Type:*
boolean



*Default:*
` false `



## languages\.python\.poetry\.install\.compile



Whether ` poetry install ` should compile Python source files to bytecode\.



*Type:*
boolean



*Default:*
` false `



## languages\.python\.poetry\.install\.extras



Which extras to install\. See ` --extras `\.



*Type:*
list of string



*Default:*
` [ ] `



## languages\.python\.poetry\.install\.groups



Which dependency groups to install\. See ` --with `\.



*Type:*
list of string



*Default:*
` [ ] `



## languages\.python\.poetry\.install\.ignoredGroups



Which dependency groups to ignore\. See ` --without `\.



*Type:*
list of string



*Default:*
` [ ] `



## languages\.python\.poetry\.install\.installRootPackage



Whether the root package (your project) should be installed\. See ` --no-root `



*Type:*
boolean



*Default:*
` false `



## languages\.python\.poetry\.install\.onlyGroups



Which dependency groups to exclusively install\. See ` --only `\.



*Type:*
list of string



*Default:*
` [ ] `



## languages\.python\.poetry\.install\.onlyInstallRootPackage



Whether to only install the root package (your project) should be installed, but no dependencies\. See ` --only-root `



*Type:*
boolean



*Default:*
` false `



## languages\.python\.poetry\.install\.quiet



Whether ` poetry install ` should avoid outputting messages during devenv initialisation\.



*Type:*
boolean



*Default:*
` false `



## languages\.python\.poetry\.install\.verbosity



What level of verbosity the output of ` poetry install ` should have\.



*Type:*
one of “no”, “little”, “more”, “debug”



*Default:*
` "no" `



## languages\.python\.uv\.enable



Whether to enable uv\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## languages\.python\.uv\.package



The uv package to use\.



*Type:*
package



*Default:*
` pkgs.uv `



## languages\.python\.venv\.enable



Whether to enable Python virtual environment\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## languages\.python\.venv\.quiet



Whether ` pip install ` should avoid outputting messages during devenv initialisation\.



*Type:*
boolean



*Default:*
` false `



## languages\.python\.venv\.requirements



Contents of pip requirements\.txt file\.
This is passed to ` pip install -r ` during ` devenv shell ` initialisation\.



*Type:*
null or strings concatenated with “\\n” or path



*Default:*
` null `



## languages\.python\.version



The Python version to use\.
This automatically sets the ` languages.python.package ` using [nixpkgs-python](https://github\.com/cachix/nixpkgs-python)\.



*Type:*
null or string



*Default:*
` null `



*Example:*
` "3.11 or 3.11.2" `
