#!/usr/bin/env python
import sys
import os

print("Verifying sys.base_prefix points to wrapped python...")
print("sys.base_prefix:", sys.base_prefix)
print("sys.executable:", sys.executable)

# Check that sys.base_prefix points to the -env buildEnv, not the bare interpreter
assert "-env" in sys.base_prefix, (
    f"sys.base_prefix ({sys.base_prefix}) should point to python-env with packages, not bare interpreter"
)

# Verify packages from withPackages are accessible from base_prefix
site_packages = os.path.join(
    sys.base_prefix,
    "lib",
    f"python{sys.version_info.major}.{sys.version_info.minor}",
    "site-packages",
)
matplotlib_path = os.path.join(site_packages, "matplotlib")
assert os.path.exists(matplotlib_path), (
    f"matplotlib should exist in base_prefix site-packages at {matplotlib_path}, but it doesn't"
)

print("✓ sys.base_prefix correctly points to wrapped python with packages")

print("Testing imports from Nix's withPackages...")
import matplotlib

print("✓ matplotlib works!")
import numpy

print("✓ numpy works!")
import IPython

print("✓ ipython works!")
import tkinter

print("✓ tkinter works!")

print("Testing imports from the Python env...")
import requests

print("✓ requests works!")
import pytest

print("✓ pytest works!")
