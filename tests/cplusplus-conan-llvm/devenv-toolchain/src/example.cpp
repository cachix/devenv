#include <iostream>

#include "example.h"

void example_print_vector(const std::vector<std::string> &strings) {
  for (std::vector<std::string>::const_iterator it = strings.begin();
       it != strings.end(); ++it) {
    std::cout << "example/0.0.1 " << *it << std::endl;
  }
}
