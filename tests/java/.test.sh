#!/usr/bin/env bash
set -euo pipefail

echo "JAVA_HOME=$JAVA_HOME"
java_version=$(java -version 2>&1)
echo "$java_version"
grep -Eq 'version "17[."]' <<< "$java_version"
test "$(readlink -f "$(command -v java)")" = "$(readlink -f "$JAVA_HOME/bin/java")"

maven_version=$(mvn --version)
echo "$maven_version"
grep -Eq 'Java version: 17[.,[:space:]]' <<< "$maven_version"

gradle_version=$(gradle --version)
echo "$gradle_version"
grep -Eq '^(Launcher )?JVM:[[:space:]]+17([.[:space:]]|$)' <<< "$gradle_version"

project_dir=$(mktemp -d)
trap 'rm -rf "$project_dir"' EXIT
cp pom.xml build.gradle settings.gradle "$project_dir/"
cp -R src "$project_dir/"
export GRADLE_USER_HOME="$project_dir/gradle-home"

mvn --batch-mode --no-transfer-progress \
  -Dmaven.repo.local="$project_dir/m2" -f "$project_dir/pom.xml" test
gradle --no-daemon --console=plain --project-dir "$project_dir" test
