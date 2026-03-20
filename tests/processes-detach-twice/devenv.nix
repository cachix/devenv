# Test that running devenv up -d twice should fail when processes are already running
{
  process.manager.implementation = "native";
  processes.dummy.exec = "sleep 3600";
}
