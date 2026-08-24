## JDK

To change the JDK used by the Scala tools (Coursier, Scalafmt, sbt, Mill, scala-cli), set the [`languages.java.jdk.package`](java.md#languagesjavajdkpackage) option.

Metals requires Java 17 or newer to run. It uses `languages.java.jdk.package` too, but only when that JDK is 17 or newer; otherwise it falls back to `pkgs.jdk`. This does not affect which JDK Metals indexes or builds your project with.

[comment]: # (Please add your documentation on top of this line)

@AUTOGEN_OPTIONS@
