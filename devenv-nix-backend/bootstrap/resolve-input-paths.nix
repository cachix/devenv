{
  src,
  names,
  lockFileJSON,
  sourceResolver ? import ./lock-sources.nix,
}:
let
  lockFilePath = src + "/devenv.lock";
  lockFile = builtins.fromJSON lockFileJSON;
  sources = sourceResolver { inherit src lockFile; };
  rootInputs = lockFile.nodes.${lockFile.root}.inputs or { };
in
if lockFile.version < 5 || lockFile.version > 7 then
  throw "lock file '${lockFilePath}' has unsupported version ${toString lockFile.version}"
else
  builtins.listToAttrs (
    map (
      name:
      if !(builtins.hasAttr name rootInputs) then
        throw "input '${name}' is missing from ${lockFilePath}"
      else
        {
          inherit name;
          value = toString (sources.nodePath (sources.resolveInput rootInputs.${name}));
        }
    ) names
  )
