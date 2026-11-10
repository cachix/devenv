{ lib }:
let
  marker = "devenv-env-prototype";
  isPlan = value: builtins.isAttrs value && (value._type or null) == marker;
  plan = contributions: { _type = marker; inherit contributions; };
  contribution = kind: value: plan [{ inherit kind value; }];
  fail = loc: message: throw "${lib.showOption loc}: ${message}";

  # Match Nix's unstructured derivation coercion, including store-copying paths.
  toEnvString = value:
    if builtins.isList value then lib.concatMapStringsSep " " toEnvString value
    else if builtins.isPath value then "${value}"
    else toString value;

  flatten = definition:
    if isPlan definition.value then
      map (item: { inherit (definition) file; inherit (item) kind value; }) definition.value.contributions
    else
      [{ inherit (definition) file value; kind = "initial"; }];

  mergeContributions = loc: items:
    let
      ofKind = kind: builtins.filter (item: item.kind == kind) items;
      single = kind:
        let definitions = map (item: { inherit (item) file value; }) (ofKind kind);
        in if definitions == [ ] then { present = false; }
        else {
          present = true;
          value =
            (if kind == "initial" then lib.types.anything.merge else lib.mergeEqualOption)
              (loc ++ [ kind ])
              definitions;
        };
      prepends = ofKind "prepend";
      appends = ofKind "append";
      extensions = prepends ++ appends;
      separators = lib.unique (map (item: item.value.separator) extensions);
      replacement = single "set";
      fallback = single "default";
      name = lib.last loc;
      namesValid = builtins.match "[A-Za-z_][A-Za-z0-9_]*" name != null
        && !(lib.hasPrefix "_devenv_env_" name);
      extensionValues = entries: lib.concatMap (item: item.value.values) entries;
    in
    assert namesValid || fail loc "helpers require a shell variable name outside the reserved _devenv_env_ namespace";
    assert builtins.length separators <= 1 || fail loc "prepend/append separators conflict";
    assert !(replacement.present && replacement.value == null)
      || fail loc "set null is ambiguous; omit the definition instead";
    assert !(fallback.present && fallback.value == null)
      || fail loc "default null is ambiguous; omit the definition instead";
    {
      initial = single "initial";
      inherit replacement fallback;
      separator = if separators == [ ] then "" else builtins.head separators;
      prepend = extensionValues prepends;
      append = extensionValues appends;
    };

  normalize = loc: value:
    if isPlan value then
      mergeContributions loc (flatten { file = "resolved environment"; inherit value; })
    else {
      initial = { present = true; inherit value; };
      replacement.present = false;
      fallback.present = false;
      separator = "";
      prepend = [ ];
      append = [ ];
    };

  render = name: resolved:
    let
      inherit (resolved) replacement fallback separator;
      prefix = lib.concatMapStringsSep separator toEnvString resolved.prepend;
      suffix = lib.concatMapStringsSep separator toEnvString resolved.append;
      quote = lib.escapeShellArg;
    in
    ''
      _devenv_env_separator=${quote separator}
      ${if replacement.present then ''
        _devenv_env_value=${quote (toEnvString replacement.value)}
      '' else ''
        _devenv_env_value="''${${name}-}"
        ${lib.optionalString fallback.present ''
          if [[ ! -v ${name} ]]; then _devenv_env_value=${quote (toEnvString fallback.value)}; fi
        ''}
      ''}
      ${lib.optionalString (resolved.prepend != [ ]) ''
        _devenv_env_value=${quote prefix}"''${_devenv_env_value:+$_devenv_env_separator}$_devenv_env_value"
      ''}
      ${lib.optionalString (resolved.append != [ ]) ''
        _devenv_env_value="$_devenv_env_value''${_devenv_env_value:+$_devenv_env_separator}"${quote suffix}
      ''}
      export ${name}="$_devenv_env_value"
    '';
in
rec {
  # These are proposed helpers, not an existing devenv API.
  beforeSetup = contribution "initial";
  set = contribution "set";
  default = contribution "default";
  prepend = separator: values: contribution "prepend" { inherit separator values; };
  append = separator: values: contribution "append" { inherit separator values; };
  combine = operations: plan (lib.concatMap
    (operation:
      if isPlan operation then operation.contributions
      else throw "env.combine expects environment helpers"
    )
    operations);

  type = lib.types.mkOptionType {
    name = "devenv-environment-value";
    description = "a legacy setup value or explicit environment operations";
    check = _: true;
    merge = loc: definitions:
      if builtins.any (definition: isPlan definition.value) definitions then
        let
          items = lib.concatMap flatten definitions;
          resolved = mergeContributions loc items;
          # Validate the merge without recursively forcing package derivations.
          initial = if resolved.initial.present then resolved.initial.value else null;
        in
        builtins.seq resolved (builtins.seq initial
          (plan (map (item: { inherit (item) kind value; }) items)))
      else lib.types.anything.merge loc definitions;
  };

  resolve = environment:
    let
      resolved = lib.mapAttrs (name: value: normalize [ "env" name ] value) environment;
      initial = lib.mapAttrs (_: value: value.initial.value)
        (lib.filterAttrs (_: value: value.initial.present) resolved);
      final = lib.filterAttrs
        (_: value:
          value.replacement.present || value.fallback.present
          || value.prepend != [ ] || value.append != [ ]
        )
        resolved;
    in
    {
      inherit initial final;
      finalScript = lib.optionalString (final != { }) ''
        _devenvFinalizeEnv() {
          local _devenv_env_value _devenv_env_separator
          ${lib.concatStrings (lib.mapAttrsToList render final)}
        }
        _devenvFinalizeEnv
        unset -f _devenvFinalizeEnv
      '';
    };
}
