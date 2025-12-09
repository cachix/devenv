{ pkgs, ... }: {
  name = "copytoroot-test";

  # Create test files and directories
  files = {
    "test-file.txt".text = "This is a test file";
    "test-dir/file1.txt".text = "File 1 in directory";
    "test-dir/file2.txt".text = "File 2 in directory";
  };

  containers = {
    # Test copying a directory
    test-dir = {
      name = "test-dir";
      copyToRoot = ./test-dir;
      startupCommand = "ls -la /env";
    };

    # Test copying a single file
    test-file = {
      name = "test-file";
      copyToRoot = ./test-file.txt;
      startupCommand = "ls -la /env";
    };

    # Test copying multiple paths (list)
    test-multiple = {
      name = "test-multiple";
      copyToRoot = [ ./test-file.txt ./test-dir ];
      startupCommand = "ls -la /env";
    };
  };
}
