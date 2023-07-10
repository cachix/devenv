import json
import sys
import os
from pathlib import Path

from strictyaml import Map, MapPattern, Str, Seq
from strictyaml import load, Bool, Any, Optional, YAMLError


inputsSchema = MapPattern(Str(), Map({
      "url": Str(),
      Optional("flake", default=None): Bool(),
      Optional("inputs", default=None): Any(),
      Optional("overlays", default=None): Seq(Str())
}))

schema = Map({
      Optional("inputs", default=None): inputsSchema,
      Optional("allowUnfree", default=False): Bool(),
      Optional("imports", default=None): Seq(Str()),
      Optional("permittedInsecurePackages", default=None): Seq(Str())
})

def validate_and_parse_yaml(dot_devenv_root):
  try:
      with open(Path("devenv.yaml")) as f:
          devenv = load(f.read(), schema, label="devenv.yaml").data
  except YAMLError as error:
      print("Validation error in `devenv.yaml`", error)
      sys.exit(1)

  inputs = {}
  for input, attrs in devenv.get('inputs', {}).items():
      inputs[input] = {k: attrs[k] for k in ('url', 'inputs', 'flake')
                       if k in attrs}

  with open(os.path.join(dot_devenv_root, "flake.json"), 'w') as f:
      f.write(json.dumps(inputs))

  with open(os.path.join(dot_devenv_root, "devenv.json"), 'w') as f:
      f.write(json.dumps(devenv))

  with open(os.path.join(dot_devenv_root, "imports.txt"), 'w') as f:
      f.write("\n".join(devenv.get('imports', [])))