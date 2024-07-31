  # Ruby
  


## languages\.ruby\.enable



Whether to enable tools for Ruby development\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## languages\.ruby\.package



The Ruby package to use\.



*Type:*
package



*Default:*
` pkgs.ruby_3_1 `



## languages\.ruby\.bundler\.enable

Whether to enable bundler\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## languages\.ruby\.bundler\.package



The bundler package to use\.



*Type:*
package



*Default:*
` pkgs.bundler.override { ruby = cfg.package; } `



## languages\.ruby\.version



The Ruby version to use\.
This automatically sets the ` languages.ruby.package ` using [nixpkgs-ruby](https://github\.com/bobvanderlinden/nixpkgs-ruby)\.



*Type:*
null or string



*Default:*
` null `



*Example:*
` "3.2.1" `



## languages\.ruby\.versionFile



The \.ruby-version file path to extract the Ruby version from\.
This automatically sets the ` languages.ruby.package ` using [nixpkgs-ruby](https://github\.com/bobvanderlinden/nixpkgs-ruby)\.
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
