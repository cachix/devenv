{
  enterTest = ''
    pushd projectB
      devenv shell python -- --version
      devenv shell cargo -- --version
    popd
  '';
}
