{
  integrations.gitnr.".gitignore" = {
    enableDefaultTemplates = false;
    templates = [
      "file:./template.gitignore"
    ];
    content = [
      "*.log"
      "dist/"
    ];
  };
}
