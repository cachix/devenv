{
  env.TEST_DEFAULT = "1";
  processes.default.exec = "echo ${builtins.readFile ./example.txt}";
}
