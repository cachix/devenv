{
  outputs = { ... }: {
    overlays.default = self: super: {
      hello2 = self.hello;
    };
  };
}
