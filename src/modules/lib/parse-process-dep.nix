{ lib }:

# Parse a dependency string like "devenv:processes:foo@ready" into
# { name = "foo"; suffix = "ready"; } or null if it's not a process dep.
# Valid suffixes: "ready" (default), "complete".
dep:
let
  parts = lib.splitString "@" dep;
  rawName = builtins.head parts;
  suffix = if builtins.length parts > 1 then builtins.elemAt parts 1 else "ready";
  prefix = "devenv:processes:";
in
assert builtins.length parts <= 2
  || throw "Invalid process dependency '${dep}': expected at most one '@' separator";
if lib.hasPrefix prefix rawName then
  {
    name = lib.removePrefix prefix rawName;
    inherit suffix;
    # Map devenv suffix to process-compose depends_on condition.
    # "ready" -> wait for healthy (readiness probe passed);
    # "complete" -> wait for started (process-compose will see it exit for oneshots).
    pcCondition = if suffix == "complete" then "process_started" else "process_healthy";
  }
else
  null
