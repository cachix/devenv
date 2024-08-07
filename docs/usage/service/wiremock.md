

[comment]: # (Please add your documentation on top of this line)

## services\.wiremock\.enable



Whether to enable WireMock\.



*Type:*
boolean



*Default:*
` false `



*Example:*
` true `



## services\.wiremock\.package



Which package of WireMock to use\.



*Type:*
package



*Default:*
` pkgs.wiremock `



## services\.wiremock\.disableBanner

Whether to disable print banner logo\.



*Type:*
boolean



*Default:*
` false `



## services\.wiremock\.mappings



The mappings to mock\.
See the JSON examples on [https://wiremock\.org/docs/stubbing/](https://wiremock\.org/docs/stubbing/) for more information\.



*Type:*
JSON value



*Default:*
` [ ] `



*Example:*

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



## services\.wiremock\.port



The port number for the HTTP server to listen on\.



*Type:*
signed integer



*Default:*
` 8080 `



## services\.wiremock\.verbose



Whether to log verbosely to stdout\.



*Type:*
boolean



*Default:*
` false `
