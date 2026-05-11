{ pkgs, ... }:

{
  # This test demonstrates using permitted_unfree_packages
  # to allow specific unfree packages by name
  packages = [
    pkgs.terraform # This is an unfree package
  ];

  enterTest = ''
    echo "Testing permitted_unfree_packages functionality"
    echo "Terraform (unfree package) should be available:"
    if ! terraform version; then
      echo "ERROR: Terraform not found"
      exit 1
    fi
    echo "SUCCESS: Terraform is available"
  '';
}
