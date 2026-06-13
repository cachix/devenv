#include "hello-world.h"
#include <vector>
#include <string>

int main() {
    hello_world();

    std::vector<std::string> vec;
    vec.push_back("test_package");

    hello_world_print_vector(vec);
}
