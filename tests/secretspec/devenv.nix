{ pkgs, config, ... }:

{
  # Test that secrets are available in Nix
  enterShell = ''
    # Expected JSON structure based on .env values
    expected_json='{"TEST_API_KEY":"test-api-key-123","TEST_DATABASE_URL":"postgresql://test:test@localhost/testdb","TEST_OPTIONAL":"optional-value"}'

    # Actual JSON from config
    actual_json='${builtins.toJSON config.secretspec.secrets}'

    # Print both for comparison
    echo "Expected JSON:"
    echo "$expected_json"
    echo ""
    echo "Actual JSON:"
    echo "$actual_json"
    echo ""

    # Assert they match
    if [ "$expected_json" = "$actual_json" ]; then
      echo "✓ JSON assertion passed: secrets match expected structure"
    else
      echo "✗ JSON assertion failed: secrets don't match expected structure"
      exit 1
    fi
  '';
}
