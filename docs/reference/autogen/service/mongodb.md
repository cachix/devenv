  # Mongodb
  


## services\.mongodb\.enable



Whether to enable MongoDB process and expose utilities\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## services\.mongodb\.package



Which MongoDB package to use\.



*Type:*
package



*Default:*
` pkgs.mongodb `



## services\.mongodb\.additionalArgs

Additional arguments passed to ` mongod `\.



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



## services\.mongodb\.initDatabasePassword



This used in conjunction with initDatabaseUsername, create a new user and set that user’s password\. This user is created in the admin authentication database and given the role of root, which is a “superuser” role\.



*Type:*
string



*Default:*
` "" `



*Example:*
` "secret" `



## services\.mongodb\.initDatabaseUsername



This used in conjunction with initDatabasePassword, create a new user and set that user’s password\. This user is created in the admin authentication database and given the role of root, which is a “superuser” role\.



*Type:*
string



*Default:*
` "" `



*Example:*
` "mongoadmin" `
