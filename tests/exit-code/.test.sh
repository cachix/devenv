# Verify that the command returns the correct exit code
devenv shell -- bash -c 'exit 42'
status=$?

if [ "$status" -ne 42 ]; then
    echo "Test failed: expected exit code 42, got $status"
    echo "The shell did not pass the exit code correctly."
    exit 1
fi
