{ lib }:

# Parse a dependency string like "devenv:processes:foo@ready" into
# { name = "foo"; suffix = "ready"; } or null if it's not a process dep.
# Valid suffixes: "started", "ready" (default), "completed".
dep:
let
  parts = lib.splitString "@" dep;
  rawName = builtins.head parts;
  suffix = if builtins.length parts > 1 then builtins.elemAt parts 1 else "ready";
  prefix = "devenv:processes:";
  validSuffixes = [ "started" "ready" "completed" ];
  pcConditions = {
    "started" = "process_started";
    "ready" = "process_healthy";
    "completed" = "process_completed";
  };
in
assert builtins.length parts <= 2
  || throw "Invalid process dependency '${dep}': expected at most one '@' separator";
assert builtins.elem suffix validSuffixes
  || throw "Invalid suffix '@${suffix}' in dependency '${dep}': must be one of @started, @ready, @completed";
if lib.hasPrefix prefix rawName then
  {
    name = lib.removePrefix prefix rawName;
    inherit suffix;
    pcCondition = pcConditions.${suffix};
  }
else
  null
