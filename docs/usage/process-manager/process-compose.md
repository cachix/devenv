

[comment]: # (Please add your documentation on top of this line)

## process-managers\.process-compose\.enable

Whether to enable process-compose as process-manager\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## process-managers\.process-compose\.package



The process-compose package to use\.



*Type:*
package



*Default:*
` pkgs.process-compose `



## process-managers\.process-compose\.settings



process-compose\.yaml specific process attributes\.

Example: https://github\.com/F1bonacc1/process-compose/blob/main/process-compose\.yaml\`



*Type:*
YAML value



*Default:*
` { } `



*Example:*

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
