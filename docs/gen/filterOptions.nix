# Filter NixOS module options by a predicate.
#
# https://gitlab.com/rycee/nur-expressions/-/blob/master/doc/default.nix
# https://github.com/molybdenumsoftware/pr-tracker/blob/main/filterOptions.nix
{
  concatMapAttrs,
  isOption,
  ...
}:
predicate: options:
let
  inherit (builtins) isAttrs;

  recurse =
    path: options:
    concatMapAttrs (
      name: value:
      let
        newPath = path ++ [ name ];
      in
      # If the value is a submodule, recurse into the submodule options.
      if (isOption value && value.type.name == "submodule") then
        {
          ${name} = value // {
            type = value.type // {
              getSubOptions = loc: recurse newPath (value.type.getSubOptions loc);
            };
          };
        }
      # Recurse into non-option attrs in search of more options.
      else if (isAttrs value && !(isOption value)) then
        { ${name} = recurse newPath value; }
      # Test the predicate on the value.
      else if predicate newPath value then
        { ${name} = value; }
      else
        { }
    ) options;
in
recurse [ ] options
