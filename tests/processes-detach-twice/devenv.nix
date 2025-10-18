# Test that running devenv up -d twice should fail when processes are already running
{
  processes.dummy.exec = "sleep 60";
}
