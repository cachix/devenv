{ buildGoModule, fetchFromGitHub, lib }:

buildGoModule rec {
  pname = "dang";
  # version = "2d592c8d71007a5242c0f18c6220fedd3423eee5";
  version = "v2.1.4";

  src = fetchFromGitHub {
    owner = "vito";
    repo = "dang";
    rev = version;
    hash = "sha256-ZED7rVmC/0l2duMPYFuiuir5jCvb5Rqsk846sh9Kk5I=";
  };

  vendorHash = "sha256-mFzpZ5mxo1mkUsU7rFkT2u6KI7GQS9cE6lRYH3q5KGI=";
  proxyVendor = true;

  doCheck = false;
  subPackages = [ "cmd/dang" ];

  meta = {
    description = "Experimental GraphQL scripting language";
    homepage = "https://github.com/vito/dang";
    license = lib.licenses.asl20;
    platforms = lib.platforms.unix;
    mainProgram = "dang";
  };
}
