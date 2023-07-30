#!/bin/sh
set -ex

make main
./main

make main-c++
./main-c++
