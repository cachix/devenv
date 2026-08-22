{ inputs, ... }:

{
  assertions = [
    {
      assertion = inputs ? remote-tool;
      message = "remote-tool from the imported devenv.yaml was not merged";
    }
    {
      assertion = inputs ? nested-tool;
      message = "nested-tool from the transitive remote devenv.yaml was not merged";
    }
  ];

  enterTest = ''
    test "$REMOTE_MODULE" = "remote-tool"
    test "$REMOTE_NESTED_MODULE" = "nested-tool"
  '';
}
