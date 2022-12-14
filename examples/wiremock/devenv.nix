{ ... }:

{
  services.wiremock = {
    enable = true;
    mappings = [
      {
        request = {
          method = "GET";
          url = "/";
        };
        response = {
          status = 200;
          headers."Content-Type" = "text/plain";
          body = "Hello World!";
        };
      }
    ];
  };
}
