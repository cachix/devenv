set -e

# GNAT
gnat make test_hello_world.adb
./test_hello_world

# GPRBuild
gprbuild -P ./gprbuild/gprbuild.gpr
./gprbuild/obj/main

# Alire
alr get -b hello
pushd hello*
alr run
popd

alr -n init --bin test_hello_with_deps > /dev/null
cp test_hello_with_deps.adb test_hello_with_deps/src/test_hello_with_deps.adb
alr --chdir=test_hello_with_deps with gnatcoll_minimal
alr --chdir=test_hello_with_deps run

