{ pkgs }:

let
  pythonPackages = pkgs.python3Packages;
in
pythonPackages.buildPythonPackage rec {
  pname = "tesh";
  version = "0.1";

  format = "pyproject";

  src = pkgs.fetchFromGitHub {
    owner = "OceanSprint";
    repo = "tesh";
    rev = "a5c84592977188a321a0be434b2f5f4ca0843a4b";
    sha256 = "sha256-mIL9LhfvVUlXbTMkSe87lUm0Ft933zFCU72y9kSr8MU=";
  };

  checkInputs = [ pythonPackages.pytest ];
  propagatedBuildInputs = [ pythonPackages.poetry pythonPackages.click pythonPackages.pexpect ];
}
