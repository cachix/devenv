{ pkgs, ... }:

{
  languages.java.jdk.package = pkgs.jdk11;
  languages.scala.enable = true;
}
