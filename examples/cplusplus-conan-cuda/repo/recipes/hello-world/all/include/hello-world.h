#pragma once

#include <vector>
#include <string>


#ifdef _WIN32
  #define HELLO_WORLD_EXPORT __declspec(dllexport)
#else
  #define HELLO_WORLD_EXPORT
#endif

HELLO_WORLD_EXPORT void hello_world();
HELLO_WORLD_EXPORT void hello_world_print_vector(const std::vector<std::string> &strings);
