{ config, lib, pkgs, ... }:

let
  markerDir = "${config.devenv.state}/markers";
in
{
  processes.watched = {
    exec = ''
      mkdir -p ${markerDir}
      count=0
      if [ -f ${markerDir}/watched.count ]; then
        count=$(cat ${markerDir}/watched.count)
      fi
      count=$((count + 1))
      echo "$count" > ${markerDir}/watched.count
      echo "watched: start number $count"
      exec sleep infinity
    '';
    watch = {
      paths = [ ./watch-target ];
    };
  };

  # Assert at Nix eval time that watch paths are source paths, not store paths.
  # Without the toString fix, ./watch-target would be copied to /nix/store and
  # the file watcher would monitor the immutable store copy instead of the source.
  assertions =
    let
      taskConfig = config.tasks."devenv:processes:watched";
      watchPath = builtins.head taskConfig.process.watch.paths;
    in
    [
      {
        assertion = !(lib.hasPrefix "/nix/store" watchPath);
        message = "watch path should be a source path, not a store path: ${watchPath}";
      }
    ];

  enterTest = ''
    wait_for_processes

    echo "--- Process started ---"
    if [ -f ${markerDir}/watched.count ]; then
      echo "PASS: watched process started (count: $(cat ${markerDir}/watched.count))"
    else
      echo "FAIL: watched process did not start"
      exit 1
    fi

    echo "--- Triggering file change in watched source dir ---"
    echo "change-$(date +%s)" > ./watch-target/trigger.txt

    echo "--- Waiting for restart ---"
    timeout=30
    elapsed=0
    while [ $elapsed -lt $timeout ]; do
      if [ -f ${markerDir}/watched.count ]; then
        count=$(cat ${markerDir}/watched.count)
        if [ "$count" -ge 2 ]; then
          echo "PASS: process restarted after file change (count: $count)"
          exit 0
        fi
      fi
      sleep 1
      elapsed=$((elapsed + 1))
    done

    echo "FAIL: process did not restart within $timeout seconds"
    exit 1
  '';
}
