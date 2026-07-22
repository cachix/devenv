{ pkgs, lib, ... }:
{
  languages.cplusplus = {
    enable = true;
    conan = {
      enable = true;
      install.enable = true;
      config = {
        stdenv = pkgs.cudaPackages_13_2.backendStdenv;
        devShell = {
          tools = {
            inherit (pkgs.cudaPackages_13_2)
              cuda_nvcc
              cuda_cccl
              cuda_cudart
              cuda_nvrtc
              cuda_nvtx
              cuda_profiler_api
              cuda_cuxxfilt
              libcublas
              libnvfatbin
              libnvptxcompiler;
          };
          env = {
            LD_LIBRARY_PATH = "/usr/lib/wsl/lib";
            MESA_D3D12_DEFAULT_ADAPTER_NAME = "NVIDIA";
            GALLIUM_DRIVER = "d3d12";
          };
        };
        profiles = {
          settings.compiler = {
            "compiler.cppstd" = "20";
          };
          settings._.build_type = "Release";
          runEnv = [
            {
              name = "LD_LIBRARY_PATH";
              op = "+=(path)";
              value = "/usr/lib/wsl/lib";
            }
            {
              name = "MESA_D3D12_DEFAULT_ADAPTER_NAME";
              op = "=";
              value = "NVIDIA";
            }
            {
              name = "GALLIUM_DRIVER";
              op = "=";
              value = "d3d12";
            }
          ];
        };
        remotes.local = {
          url = "./repo";
          local = true;
          allowedPackages = [ "hello-world/0.0.1.cci.20260428" ];
        };
      };
    };
  };
}
