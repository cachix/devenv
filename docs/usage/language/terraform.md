  # Terraform
  


## languages\.terraform\.enable

Whether to enable tools for Terraform development\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## languages\.terraform\.package



The Terraform package to use\.



*Type:*
package



*Default:*
` pkgs.terraform `



## languages\.terraform\.version



The Terraform version to use\.
This automatically sets the ` languages.terraform.package ` using [nixpkgs-terraform](https://github\.com/stackbuilders/nixpkgs-terraform)\.



*Type:*
null or string



*Default:*
` null `



*Example:*
` "1.5.0 or 1.6.2" `
