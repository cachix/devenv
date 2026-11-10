{ lib }:
let
  marker = "devenv-env";
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
      prepends = builtins.filter (item: item.value.values != [ ]) (ofKind "prepend");
      appends = builtins.filter (item: item.value.values != [ ]) (ofKind "append");
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

  # This is the configured portion of a value, not the environment produced by
  # running setup hooks. Preserve native types when no composition is needed.
  configuredValue = resolved:
    let
      base =
        if resolved.replacement.present then resolved.replacement.value
        else if resolved.initial.present then resolved.initial.value
        else if resolved.fallback.present then resolved.fallback.value
        else "";
    in
    if resolved.prepend == [ ] && resolved.append == [ ] then base
    else
      lib.concatStringsSep resolved.separator (
        (map toEnvString resolved.prepend)
        ++ lib.optional (base != null && toEnvString base != "") (toEnvString base)
        ++ (map toEnvString resolved.append)
      );
in
rec {
  beforeSetup = contribution "initial";
  set = contribution "set";
  default = contribution "default";
  prepend = separator: values:
    assert builtins.isString separator && builtins.isList values;
    contribution "prepend" { inherit separator values; };
  append = separator: values:
    assert builtins.isString separator && builtins.isList values;
    contribution "append" { inherit separator values; };
  combine = operations: plan (lib.concatMap
    (operation:
      if isPlan operation then operation.contributions
      else throw "env.combine expects environment helpers"
    )
    operations);

  type = lib.types.mkOptionType {
    name = "devenv-environment-value";
    description = "a setup value or explicit environment operations";
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

  # Keep config.env usable by modules that interpolate values or serialize it.
  # Only the shell construction consumes the operation metadata.
  values = environment: lib.mapAttrs
    (name: value: if isPlan value then configuredValue (normalize [ "env" name ] value) else value)
    environment;

  resolve = environment:
    let
      resolved = lib.mapAttrs (name: value: normalize [ "env" name ] value) environment;
      helpers = lib.filterAttrs (_: isPlan) environment;
      initial = lib.mapAttrs
        (name: value:
          if helpers ? ${name} && value.initial.value != null
          then toEnvString value.initial.value
          else value.initial.value)
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
      hasHelpers = helpers != { };
      initialScript = lib.optionalString (helpers != { }) (
        let names = builtins.filter (name: initial ? ${name} && initial.${name} != null) (builtins.attrNames helpers);
        in lib.optionalString (names != [ ]) "export ${lib.concatStringsSep " " names}\n"
      );
      finalScript = lib.optionalString (final != { }) ''
        _devenv_env_finalize() {
          local _devenv_env_value _devenv_env_separator
          ${lib.concatStrings (lib.mapAttrsToList render final)}
        }
        _devenv_env_finalize
        unset -f _devenv_env_finalize
        # Activation can filter sandbox values or append the caller's PATH.
        # Carry the computed values in a non-exported array, then restore them
        # before enterShell. This also works with nix develop and older CLIs.
        declare -gA _devenv_env_final_values=()
        ${lib.concatStrings (lib.mapAttrsToList (name: _: ''
          _devenv_env_final_values[${lib.escapeShellArg name}]="''${${name}}"
        '') final)}
        shellHook=${lib.escapeShellArg ''
          for _devenv_env_name in "''${!_devenv_env_final_values[@]}"; do
            export "$_devenv_env_name=''${_devenv_env_final_values[$_devenv_env_name]}"
          done
          unset _devenv_env_name _devenv_env_final_values
        ''}"''${shellHook-}"
      '';
    };

  wrapShell = { pkgs, environment, shell }:
    let
      resolved = resolve environment;
      wrap = shell:
        let
          original = shell.stdenv;
          wrapper = pkgs.runCommandLocal "devenv-stdenv" { } ''
            mkdir -p "$out"
            cp -rs ${original}/. "$out/"
            rm "$out/setup"
            cat > "$out/setup" <<'EOF'
            _devenv_env_source() {
              local _devenv_env_path_name="$1"Path
              if [[ -n "''${!_devenv_env_path_name-}" ]]; then
                source "''${!_devenv_env_path_name}"
              else
                # Structured attributes already hold scripts in shell variables.
                eval "''${!1}"
              fi
            }
            _devenv_env_source _devenv_env_initial
            source ${original}/setup
            if [[ -n "''${IN_NIX_SHELL-}" ]]; then
              _devenv_env_source _devenv_env_final
              unset _devenv_env_initial _devenv_env_final _devenv_env_initialPath _devenv_env_finalPath
              unset -f _devenv_env_source
            fi
            EOF
          '';
          stdenv = original // { inherit (wrapper) outPath drvPath; };
          wrapped = lib.overrideDerivation shell (_: {
            inherit stdenv;
            _devenv_env_initial = resolved.initialScript;
            _devenv_env_final = resolved.finalScript;
            passAsFile = (shell.drvAttrs.passAsFile or [ ]) ++ [ "_devenv_env_initial" "_devenv_env_final" ];
            # mkShell records exported values in its output to retain their
            # store references. Apply final values only inside that subshell:
            # replacing PATH must not prevent the builder from running phases.
            buildPhase = ''
              (
                _devenv_env_source _devenv_env_final
                ${shell.drvAttrs.buildPhase}
              )
            '';
          });
        in
        wrapped
        // lib.optionalAttrs (shell ? overrideAttrs) { overrideAttrs = f: wrap (shell.overrideAttrs f); }
        // lib.optionalAttrs (shell ? override) { override = args: wrap (shell.override args); }
        // lib.optionalAttrs (shell ? overrideDerivation) { overrideDerivation = f: wrap (shell.overrideDerivation f); };
    in
    assert !resolved.hasHelpers || lib.all (name: !(lib.hasPrefix "_devenv_env_" name)) (builtins.attrNames environment)
      || throw "env: the _devenv_env_ namespace is reserved for environment helpers";
    if resolved.hasHelpers then wrap shell else shell;
}
