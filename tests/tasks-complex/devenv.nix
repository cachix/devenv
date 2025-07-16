{
  tasks = {
    "frontend:build" = {
      exec = "echo 'Building frontend...'";
      after = [ "frontend:test" ];
      execIfModified = [ "src/frontend/*.js" "src/frontend/*.css" ];
    };

    "frontend:test" = {
      exec = "echo 'Testing frontend...'";
      after = [ "frontend:lint" ];
      status = "test -f .frontend-test-passed";
    };

    "frontend:lint" = {
      exec = "echo 'Linting frontend...'";
    };

    "backend:build" = {
      exec = "echo 'Building backend...'";
      after = [ "backend:test" ];
      execIfModified = [ "src/backend/**/*.py" ];
    };

    "backend:test" = {
      exec = "echo 'Testing backend...'";
      after = [ "backend:lint" ];
    };

    "backend:lint" = {
      exec = "echo 'Linting backend...'";
      status = "which ruff";
    };

    "deploy:production" = {
      exec = "echo 'Deploying to production...'";
      after = [ "frontend:build" "backend:build" ];
    };

    "docs:generate" = {
      exec = "echo 'Generating documentation...'";
      execIfModified = [ "docs/**/*.md" ];
    };

    "docs:publish" = {
      exec = "echo 'Publishing documentation...'";
      after = [ "docs:generate" ];
    };
  };
}
