  # Solidity
  


## languages\.solidity\.enable

Whether to enable tools for Solidity development\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## languages\.solidity\.package



Which compiler of Solidity to use\.



*Type:*
package



*Default:*
` pkgs.elixir `



## languages\.solidity\.foundry\.enable



Whether to enable install Foundry\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## languages\.solidity\.foundry\.package



Which Foundry package to use\.



*Type:*
package



*Default:*
` foundry.defaultPackage.$${pkgs.stdenv.system} `
