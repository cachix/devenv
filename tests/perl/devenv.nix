_:

{
  languages.perl.enable = true;
  languages.perl.packages = [ "Mojolicious" "Text::Markdown::Hoedown" ];
  enterShell = ''
    perl -MText::Markdown::Hoedown -Mojo -e 'say c(1,2,markdown("hey"))->join(" ")'
  '';
}
