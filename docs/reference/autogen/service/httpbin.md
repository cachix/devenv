  # Httpbin
  


## services\.httpbin\.enable



Whether to enable httpbin\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## services\.httpbin\.bind

Addresses for httpbin to listen on\.



*Type:*
list of string



*Default:*

```
[
  "127.0.0.1:8080"
]
```



## services\.httpbin\.extraArgs



Gunicorn CLI arguments for httpbin\.



*Type:*
list of string



*Default:*
` [ ] `
