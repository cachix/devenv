#include <string>
#include <vector>

#include "example.h"
#include "hello-world.h"


int main() {
  hello_world();

  std::vector<std::string> vec;
  vec.push_back("test_package");

  example_print_vector(vec);
}
