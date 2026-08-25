{ pkgs, ... }:

{
  services.cockroachdb = {
    enable = pkgs.stdenv.hostPlatform.isLinux;
    # Avoid the Bubblewrap launcher used by buildFHSEnv in example tests,
    # where unprivileged user namespaces may be unavailable.
    package = pkgs.stdenv.mkDerivation {
      pname = "cockroachdb";
      inherit (pkgs.cockroachdb) version;

      dontUnpack = true;
      nativeBuildInputs = [ pkgs.autoPatchelfHook ];
      buildInputs = [ pkgs.stdenv.cc.cc.lib ];

      installPhase = ''
        install -Dm755 ${pkgs.cockroachdb.args.runScript} $out/bin/cockroach
        ln -s cockroach $out/bin/cockroachdb
      '';
    };
  };
}
