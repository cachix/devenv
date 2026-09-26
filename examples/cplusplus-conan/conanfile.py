from conan import ConanFile
from conan.tools.cmake import CMake, CMakeDeps, CMakeToolchain, cmake_layout


class exampleRecipe(ConanFile):
    name = "example"
    version = "0.0.1"
    package_type = "application"

    # Optional metadata
    license = "MIT"
    author = "Tarcisio G. Rodrigues"
    homepage = "https://codeberg.org/tarcisio/conan-flake"
    description = "This is the Example project."
    topics = ("testing", "example", "conan")

    # Binary configuration
    settings = "os", "compiler", "build_type", "arch"

    # Sources are located in the same place as this recipe, copy them to the recipe
    exports_sources = "CMakeLists.txt", "src/*"

    def layout(self):
        cmake_layout(self)

    def requirements(self):
        self.requires("zlib/1.3.1")

    def generate(self):
        deps = CMakeDeps(self)
        deps.generate()
        tc = CMakeToolchain(self)
        tc.generate()

    def build(self):
        cmake = CMake(self)
        cmake.configure()
        cmake.build()

    def package(self):
        cmake = CMake(self)
        cmake.install()
