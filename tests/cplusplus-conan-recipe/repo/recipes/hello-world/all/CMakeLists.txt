cmake_minimum_required(VERSION 3.15)

project(hello-world LANGUAGES CXX)

add_library(hello-world src/hello-world.cpp)

target_include_directories(hello-world PUBLIC include)
set_target_properties(hello-world PROPERTIES PUBLIC_HEADER "include/hello-world.h")
install(TARGETS hello-world)
