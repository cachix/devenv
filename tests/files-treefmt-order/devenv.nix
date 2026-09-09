{ pkgs, ... }:
{
  packages = [ pkgs.jq ];

  treefmt = {
    enable = true;

    config.programs = {
      nixfmt.enable = true;
    };
  };

  # Copy-mode files are rewritten by `devenv:files` on every shell entry, so the tree-wide
  # treefmt sweep has to wait for them: without an ordering edge the two run concurrently
  # and treefmt fails to stat a file that is being replaced.
  files."managed.yaml" = {
    text = "managed: content\n";
    copyMode = "copy";
  };
  files."linked.yaml" = {
    text = "linked: content\n";
    copyMode = "copy";
  };
  files."dirlinked.yaml" = {
    text = "dirlinked: content\n";
    copyMode = "copy";
  };

  enterTest = ''
    # The formatter walks the whole tree, so it must run after the files are in place.
    # $DEVENV_TASK_FILE is the task graph devenv runs.
    jq -e '
      map(select(.name == "devenv:treefmt:run")) as $treefmt
      | ($treefmt | length) == 1
        and ($treefmt[0].after | index("devenv:files")) != null
    ' "$DEVENV_TASK_FILE" > /dev/null || {
      echo "devenv:treefmt:run is not ordered after devenv:files"
      exit 1
    }

    # .patch.sh left a user edit here: the copy is renamed over the existing file.
    test ! -L "$DEVENV_ROOT/managed.yaml" || {
      echo "managed.yaml should not be a symlink"
      exit 1
    }
    test -w "$DEVENV_ROOT/managed.yaml" || {
      echo "managed.yaml should be writable"
      exit 1
    }
    grep -qx "managed: content" "$DEVENV_ROOT/managed.yaml" || {
      echo "managed.yaml not overwritten"
      exit 1
    }

    # .patch.sh left a symlink here: the copy replaces the link itself, so the file it
    # pointed at stays untouched.
    test ! -L "$DEVENV_ROOT/linked.yaml" || {
      echo "linked.yaml should have replaced the symlink"
      exit 1
    }
    grep -qx "linked: content" "$DEVENV_ROOT/linked.yaml" || {
      echo "linked.yaml not overwritten"
      exit 1
    }
    grep -qx "untouched" "$DEVENV_ROOT/decoy.yaml" || {
      echo "the symlink was followed instead of replaced"
      exit 1
    }

    # .patch.sh left a symlink to a directory here: `mv` resolves such a link, so the link
    # has to be unlinked first or the copy lands inside the directory.
    test ! -L "$DEVENV_ROOT/dirlinked.yaml" || {
      echo "dirlinked.yaml should have replaced the symlink"
      exit 1
    }
    test -f "$DEVENV_ROOT/dirlinked.yaml" || {
      echo "dirlinked.yaml should be a regular file"
      exit 1
    }
    grep -qx "dirlinked: content" "$DEVENV_ROOT/dirlinked.yaml" || {
      echo "dirlinked.yaml not overwritten"
      exit 1
    }
    test -z "$(ls -A "$DEVENV_ROOT/decoydir")" || {
      echo "the copy was moved into the directory the symlink pointed at"
      exit 1
    }

    # Each copy is staged in a temporary directory next to its destination and renamed over
    # it. Nothing may be left behind.
    for staging in "$DEVENV_ROOT"/.managed.yaml.* "$DEVENV_ROOT"/.linked.yaml.* \
      "$DEVENV_ROOT"/.dirlinked.yaml.*; do
      if [ -e "$staging" ]; then
        echo "staging path left behind: $staging"
        exit 1
      fi
    done
  '';
}
