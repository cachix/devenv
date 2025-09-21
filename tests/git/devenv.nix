{ config, ... }:

{
  enterTest = ''
    echo "Git root: ${toString config.git.root}"
    
    # Test that git.root is set to a valid path
    if [ "${toString config.git.root}" != "null" ]; then
      echo "Git root is set to: ${toString config.git.root}"
      # Verify it's an actual directory
      if [ -d "${toString config.git.root}" ]; then
        echo "Git root directory exists"
        # Verify it contains .git directory
        if [ -d "${toString config.git.root}/.git" ]; then
          echo "Git root contains .git directory - SUCCESS"
        else
          echo "Error: Git root does not contain .git directory"
          exit 1
        fi
      else
        echo "Error: Git root is not a valid directory"
        exit 1
      fi
    else
      echo "Error: Git root should not be null in a git repository"
      exit 1
    fi
  '';
}
