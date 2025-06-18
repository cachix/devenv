let
  inherit (builtins)
    filter
    head
    isNull
    isString
    match
    split
    ;
in
{
  # matches :: string -> string -> bool
  matches = pattern: text: !isNull (match "(${pattern})" text);
  # extractWithRegex :: string -> string -> string
  extractWithRegex =
    regex: string:
    let
      matched = match regex string;
    in
    if isNull matched then "" else head matched;
  # splitWithRegex :: string -> string -> [string]
  splitWithRegex = regex: string: filter isString (split regex string);
}
