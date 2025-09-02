{ config
, lib
, pkgs
, ...
}:

{
  # Top-level packages to the shell
  packages = [
    pkgs.jq
  ];

  # Scripts have access to the top-level `packages`
  scripts.silly-example.exec = ''echo "{\"name\":\"$1\",\"greeting\":\"Hello $1!\",\"timestamp\":\"$(date -Iseconds)\"}" | jq '';
  scripts.silly-example.description = "creates JSON with provided arg and shows it with jq";

  # Scripts can declare their own private `packages`
  scripts.serious-example.exec = ''cowsay "$*"'';
  scripts.serious-example.packages = [ pkgs.cowsay ];
  scripts.serious-example.description = ''echoes args in a very serious manner'';

  # Write scripts using your favourite language.
  scripts.python-hello.exec = ''print("Hello, world!")'';
  scripts.python-hello.package = pkgs.python3Minimal;

  # Handle custom scripts where the binary name doesn't match the package name
  scripts.nushell-greet.exec = ''
    def greet [name] {
    	["hello" $name]
    }

    greet "world"
  '';
  scripts.nushell-greet.package = pkgs.nushell;
  scripts.nushell-greet.binary = "nu";

  # Render a help section when you enter the shell, similar to `devenv info`
  enterShell = ''
    echo
    echo 🦾 Helper scripts you can run to make your development richer:
    echo 🦾
    ${pkgs.gnused}/bin/sed -e 's| |••|g' -e 's|=| |' <<EOF | ${pkgs.util-linuxMinimal}/bin/column -t | ${pkgs.gnused}/bin/sed -e 's|^|🦾 |' -e 's|••| |g'
    ${lib.generators.toKeyValue { } (lib.mapAttrs (name: value: value.description) config.scripts)}
    EOF
    echo
  '';

  # Test that the scripts work as expected with `devenv test`
  enterTest = ''
    echo "Testing silly-example"
    silly-example world | grep Hello

    echo "Testing serious-example"
    serious-example hello world | grep hello

    echo "Testing python-hello"
    python-hello | grep Hello

    echo "Testing nushell-greet"
    nushell-greet | grep hello
  '';
}
