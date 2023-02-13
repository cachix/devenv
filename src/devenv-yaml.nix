{ pkgs }:

pkgs.writers.writePython3Bin "devenv-yaml" { libraries = with pkgs.python3Packages; [ strictyaml path ]; } ''
  from strictyaml import Map, MapPattern, Str, Seq
  from strictyaml import load, Bool, Any, Optional, YAMLError
  import json
  import sys
  import os
  from path import Path

  inputsSchema = MapPattern(Str(), Map({
      "url": Str(),
      Optional("flake", default=None): Bool(),
      Optional("inputs", default=None): Any(),
      Optional("overlays", default=None): Seq(Str())
  }))

  schema = Map({
      Optional("inputs", default=None): inputsSchema,
      Optional("allowUnfree", default=False): Bool(),
      Optional("imports", default=None): Seq(Str())
  })

  filename = Path("devenv.yaml").bytes().decode('utf8')
  try:
      devenv = load(filename, schema, label="devenv.yaml").data
  except YAMLError as error:
      print("Error in `devenv.yaml`", error)
      sys.exit(1)

  inputs = {}
  for input, attrs in devenv.get('inputs', {}).items():
      inputs[input] = {k: attrs[k] for k in ('url', 'inputs', 'flake')
                       if k in attrs}

  devenv_state = sys.argv[1]

  with open(os.path.join(devenv_state, "flake.json"), 'w') as f:
      f.write(json.dumps(inputs))

  with open(os.path.join(devenv_state, "devenv.json"), 'w') as f:
      f.write(json.dumps(devenv))

  with open(os.path.join(devenv_state, "imports.txt"), 'w') as f:
      f.write(" ".join(devenv.get('imports', [])))
''
