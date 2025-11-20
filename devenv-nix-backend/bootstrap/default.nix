args@{
  # Devenv input variables
  version
, system
, # The project root (location of devenv.nix)
  devenv_root
, # The git root, if available
  git_root ? null
, # Devenv state and work directories
  devenv_dotfile
, devenv_dotfile_path
, devenv_tmpdir
, devenv_runtime
, devenv_istesting ? false
, # Direvenrc versioning
  devenv_direnvrc_latest_version
, # Container name
  container_name ? null
, # Profiles
  active_profiles ? [ ]
, hostname
, username
, # Ad-hoc options enabled via the CLI
  cli_options ? [ ]
, # Whether to skip loading the local devenv.nix
  skip_local_src ? false
, # SecretSpec data passed from Rust backend
  secretspec ? null
, # devenv.yaml configuration (inputs, imports, nixpkgs, devenv, etc.)
  devenv_config ? { }
}:

let
  inputs = (import ./resolve-lock.nix { src = devenv_root; inherit system; }).inputs;

  bootstrapLib = import ./bootstrapLib.nix { inherit inputs; };
in

bootstrapLib.mkDevenvForSystem args
