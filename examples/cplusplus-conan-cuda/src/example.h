#pragma once

#include <string>
#include <vector>

#ifdef _WIN32
#define EXAMPLE_EXPORT __declspec(dllexport)
#else
#define EXAMPLE_EXPORT
#endif

EXAMPLE_EXPORT void example_print_vector(const std::vector<std::string> &strings);
