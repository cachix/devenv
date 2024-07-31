  # Vault
  


## aws-vault\.enable



Whether to enable aws-vault integration\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## aws-vault\.package



The aws-vault package to use\.



*Type:*
package



*Default:*
` pkgs.aws-vault `



## aws-vault\.awscliWrapper

Attribute set of packages including awscli2



*Type:*
submodule



*Default:*
` pkgs `



## aws-vault\.awscliWrapper\.enable



Whether to enable Wraps awscli2 binary as ` aws-vault exec <profile> -- aws <args> `\.
\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## aws-vault\.awscliWrapper\.package



The awscli2 package to use\.



*Type:*
package



*Default:*
` pkgs.awscli2 `



## aws-vault\.opentofuWrapper



Attribute set of packages including opentofu



*Type:*
submodule



*Default:*
` pkgs `



## aws-vault\.opentofuWrapper\.enable



Whether to enable Wraps opentofu binary as ` aws-vault exec <profile> -- opentofu <args> `\.
\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## aws-vault\.opentofuWrapper\.package



The opentofu package to use\.



*Type:*
package



*Default:*
` pkgs.opentofu `



## aws-vault\.profile



The profile name passed to ` aws-vault exec `\.



*Type:*
string



## aws-vault\.terraformWrapper



Attribute set of packages including terraform



*Type:*
submodule



*Default:*
` pkgs `



## aws-vault\.terraformWrapper\.enable



Whether to enable Wraps terraform binary as ` aws-vault exec <profile> -- terraform <args> `\.
\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## aws-vault\.terraformWrapper\.package



The terraform package to use\.



*Type:*
package



*Default:*
` pkgs.terraform `



## services\.vault\.enable



Whether to enable vault process\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## services\.vault\.package



Which package of Vault to use\.



*Type:*
package



*Default:*
` pkgs.vault-bin `



## services\.vault\.address



Specifies the address to bind to for listening



*Type:*
string



*Default:*
` "127.0.0.1:8200" `



## services\.vault\.disableClustering



Specifies whether clustering features such as request forwarding are enabled



*Type:*
boolean



*Default:*
` true `



## services\.vault\.disableMlock



Disables the server from executing the mlock syscall



*Type:*
boolean



*Default:*
` true `



## services\.vault\.ui



Enables the built-in web UI



*Type:*
boolean



*Default:*
` true `
