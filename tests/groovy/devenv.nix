{
  languages.groovy.enable = true;

  enterTest = ''
    echo "1. Checking if binaries are on the PATH..."
    command -v groovy
    command -v groovy-language-server

    echo "2. Checking Groovy version..."
    groovy --version

    echo "3. Creating a test script..."
    cat > hello.groovy <<'GROOVY'
    println "hello world"
    GROOVY

    echo "4. Running the script..."
    OUTPUT=$(groovy hello.groovy)
    echo "Script output: $OUTPUT"

    if [[ "$OUTPUT" != *"hello world"* ]]; then
      echo "Error: Expected 'hello world', but got: $OUTPUT"
      exit 1
    fi

    echo "All Groovy tests passed"
  '';
}
