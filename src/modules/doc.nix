{ lib, ... }:

let
  docType = lib.types.str // {
    merge = loc: defs: defs;
  };
in
{
  options = {
    meta = {
      doc = lib.mkOption {
        type = docType;
        internal = true;
        default = "";
        description = ''
          Module documentation in markdown format.
        '';
      };
    };
  };
}
