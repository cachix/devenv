{ ... }:

{
  services.ollama = {
    enable = true;
    address = "0.0.0.0";
    port = 11434;
    loadModels = [
      "llama3.2:3b"
    ];
    # acceleration = "";
  };
}
