# Adapted from https://git.lix.systems/lix-project/flake-compat/src/branch/main/default.nix
{
  src,
  lockFilePath,
  system ? builtins.currentSystem or "unknown-system",
}:

let
  lockFile = builtins.fromJSON (builtins.readFile lockFilePath);

  rootSrc = {
    lastModified = 0;
    lastModifiedDate = formatSecondsSinceEpoch 0;
    # *hacker voice*: it's definitely a store path, I promise (actually a
    # nixlang path value, likely not pointing at the store).
    outPath = src;
  };

  # Format number of seconds in the Unix epoch as %Y%m%d%H%M%S.
  formatSecondsSinceEpoch =
    t:
    let
      rem = x: y: x - x / y * y;
      days = t / 86400;
      secondsInDay = rem t 86400;
      hours = secondsInDay / 3600;
      minutes = (rem secondsInDay 3600) / 60;
      seconds = rem t 60;

      # Courtesy of https://stackoverflow.com/a/32158604.
      z = days + 719468;
      era = (if z >= 0 then z else z - 146096) / 146097;
      doe = z - era * 146097;
      yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
      y = yoe + era * 400;
      doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
      mp = (5 * doy + 2) / 153;
      d = doy - (153 * mp + 2) / 5 + 1;
      m = mp + (if mp < 10 then 3 else -9);
      y' = y + (if m <= 2 then 1 else 0);

      pad = s: if builtins.stringLength s < 2 then "0" + s else s;
    in
    "${toString y'}${pad (toString m)}${pad (toString d)}${pad (toString hours)}${pad (toString minutes)}${pad (toString seconds)}";

  sources = import ./lock-sources.nix { inherit src lockFile rootSrc; };

  allNodes = builtins.mapAttrs (
    key: node:
    let
      sourceInfo = sources.sourceInfo key node;
      outPath = sources.nodePath key;
      inherit (sources) resolveInput;

      inputs = builtins.mapAttrs (inputName: inputSpec: allNodes.${resolveInput inputSpec}) (
        node.inputs or { }
      );

      # Only import flake.nix for non-root nodes (root doesn't need it)
      flake = if key == lockFile.root then null else import (outPath + "/flake.nix");

      outputs = if key == lockFile.root then { } else flake.outputs (inputs // { self = result; });

      # Lazy devenv evaluation for this input
      devenvEval =
        let
          bootstrapLib = import ./bootstrapLib.nix { inputs = inputs; };
        in
        bootstrapLib.mkDevenvForInput {
          input = { inherit outPath sourceInfo; };
          allInputs = inputs;
          inherit system;
        };

      result =
        outputs
        // sourceInfo
        // {
          inherit outPath;
          inherit inputs;
          inherit outputs;
          inherit sourceInfo;
          _type = "flake";
          devenv = devenvEval;
        };

      nonFlakeResult = sourceInfo // {
        inherit outPath;
        inherit inputs;
        inherit sourceInfo;
        _type = "flake";
        devenv = devenvEval;
      };

    in
    if node.flake or true && key != lockFile.root then
      assert builtins.isFunction flake.outputs;
      result
    else
      nonFlakeResult
  ) lockFile.nodes;

  result =
    if !(builtins.pathExists lockFilePath) then
      throw "${lockFilePath} does not exist"
    else if lockFile.version >= 5 && lockFile.version <= 7 then
      allNodes.${lockFile.root}
    else
      throw "lock file '${lockFilePath}' has unsupported version ${toString lockFile.version}";

in
{
  inputs = result.inputs or { } // {
    self = result;
  };
}
