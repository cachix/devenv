{
  outputs =
    { self }:
    {
      metadata = {
        inherit (self) narHash lastModified;
      };
    };
}
