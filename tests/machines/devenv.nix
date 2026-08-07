{ config, ... }: {
  # Machine with no roles: should evaluate cleanly, with every build.* output
  # falling back to the declared `null` default.
  machines.empty = { };

  # NixOS-only machine: referencing `machines.nixos.build.nixos` should fire
  # the targeted missing-input error for `disko`, since the test environment
  # doesn't declare it. `.system` stays evaluable because the error is lazy.
  machines.nixos = {
    system = "x86_64-linux";
    target.host = "root@host";
    nixos = {
      services.openssh.enable = true;
    };
  };

  # home-manager-only machine: referencing `machines.hm.build.home-manager`
  # should fire the targeted missing-input error for `home-manager`, and must
  # NOT fire the disko error, because the nixos branch is gated behind
  # `config.nixos != null` (lazy `mkIf`).
  machines.hm = {
    hardware.facter = null;
    home-manager = {
      home.username = "jdoe";
      home.homeDirectory = "/home/jdoe";
      home.stateVersion = "24.11";
    };
  };

  # nix-darwin-only machine: used to exercise the targeted missing-host error
  # path in `devenv machines deploy` (nix-darwin deployment requires SSH).
  machines.mac = {
    nix-darwin = { environment.systemPackages = [ ]; };
  };

  # NixOS machine without a target.host: used to exercise the
  # "NixOS deploys always go over SSH" error path.
  machines.nohost = {
    nixos = { services.openssh.enable = true; };
  };

  # NixOS machine with custom kexec override: used to test that the
  # install.kexec.{image, postSshPort} schema evaluates and surfaces
  # in machinesMeta.
  machines.custom-kexec = {
    system = "aarch64-linux";
    target.host = "root@arm-box.lan";
    install.kexec.image = "https://example.com/custom-kexec.tar.gz";
    install.kexec.postSshPort = 2222;
    install.copyHostKeys = true;
    nixos = { services.openssh.enable = true; };
  };

  # Machine metadata contains only the SecretSpec name and target metadata.
  # The conditional mode is a security regression sentinel: ordinary eval
  # exposes SecretSpec values by design and therefore produces 0644, while
  # `machines install` must hide values from Nix and see the valid 0600 mode.
  machines.secretful = {
    target.host = "root@secretful.example.com";
    # The secret-bearing install must force `yes` ahead of this deliberately
    # insecure value. A shell integration check captures the actual SSH argv.
    target.sshOpts = [ "-o" "StrictHostKeyChecking=no" ];
    install.secrets."/var/lib/sops-nix/key.txt" = {
      secret = "SECRET_MACHINE_AGE_KEY";
      owner = "0:0";
      mode = if config.secretspec.secrets ? SECRET_MACHINE_AGE_KEY then "0644" else "0600";
    };
    nixos = {
      services.openssh.enable = true;
      users.users.root.openssh.authorizedKeys.keys = [ "ssh-ed25519 test" ];
    };
  };

  machines.missing-secret = {
    target.host = "root@missing-secret.example.com";
    install.secrets."/var/lib/sops-nix/key.txt" = {
      secret = "UNDECLARED_MACHINE_KEY";
    };
    nixos = {
      services.openssh.enable = true;
      users.users.root.openssh.authorizedKeys.keys = [ "ssh-ed25519 test" ];
    };
  };

  # Target-side resolution must not ask the workstation's env provider for
  # this value. The production profile declares it, but the integration test
  # intentionally leaves REMOTE_MACHINE_TOKEN unset locally.
  machines.target-secretful = {
    target.host = "root@target-secretful.example.com";
    install.secretspec = {
      execution = "target";
      profile = "production";
      extraPackages = targetPkgs: [ targetPkgs.coreutils ];
    };
    install.secrets."/var/lib/bootstrap/token" = {
      secret = "REMOTE_MACHINE_TOKEN";
      owner = "0:0";
      mode = "0600";
    };
    nixos = {
      services.openssh.enable = true;
      users.users.root.openssh.authorizedKeys.keys = [ "ssh-ed25519 test" ];
    };
  };
}
