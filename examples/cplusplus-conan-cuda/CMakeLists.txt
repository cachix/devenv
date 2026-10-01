cmake_minimum_required(VERSION 3.15)
project(example CXX CUDA)

find_package(hello-world CONFIG REQUIRED)
find_package(CUDAToolkit 9.0 REQUIRED)
find_package(cuda-api-wrappers CONFIG REQUIRED)

add_executable(example src/example.cpp src/main.cpp)
target_link_libraries(example hello-world::hello-world)

add_executable(matrixMulCUBLAS src/modified_cuda_samples/matrixMulCUBLAS/matrixMulCUBLAS.cpp)
target_link_libraries(matrixMulCUBLAS cuda-api-wrappers::runtime-and-driver CUDA::cuda_driver CUDA::cudart CUDA::cublas)

install(
  TARGETS example matrixMulCUBLAS
  DESTINATION "."
  RUNTIME DESTINATION bin
  ARCHIVE DESTINATION lib
  LIBRARY DESTINATION lib)
