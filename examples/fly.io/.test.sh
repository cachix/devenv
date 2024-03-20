#!/usr/bin/env bash
if [ "$(uname -s)" == "Linux" ]; then
    devenv container shell
    devenv container processes
fi