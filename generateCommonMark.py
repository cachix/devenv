# Modified from
# https://github.com/NixOS/nixpkgs/blob/343ea4052c014657981f19b267e16122de4264e6/nixos/lib/make-options-doc/generateCommonMark.py

import json
import sys


def pretty_print_nix_types(obj):
    if isinstance(obj, dict):
        if "_type" in obj:
            _type = obj["_type"]
            if _type == "literalExpression" or _type == "literalDocBook":
                return obj["text"]

            if _type == "derivation":
                return obj["name"]

            raise Exception('Unknown type "{}" in {}'.format(_type, json.dumps(obj)))

    return obj


options = json.load(sys.stdin, object_hook=pretty_print_nix_types)
for (name, value) in options.items():
    print("##", name.replace("<", "&lt;").replace(">", "&gt;"))
    print(value["description"])
    print()
    if "type" in value:
        print("*_Type_*:")
        print(value["type"])
        print()
    print()
    if "default" in value:
        print("*_Default_*")
        print("```")
        print(json.dumps(value["default"], ensure_ascii=False, separators=(",", ":")))
        print("```")
    print()
    print()
    if "example" in value:
        print("*_Example_*")
        print("```")
        print(json.dumps(value["example"], ensure_ascii=False, separators=(",", ":")))
        print("```")
    print()
    print()
