{
  config,
  lib,
  pkgs,
  ...
}:

let
  markerDir = "${config.devenv.state}/markers";
  httpPort = config.processes.http-server.ports.http.value;
in
{
  packages = [ pkgs.curl ];

  # 1. Enabled process with readiness probe and port allocation
  processes.http-server = {
    exec = ''
      mkdir -p ${markerDir}
      touch ${markerDir}/http-server.started
      sleep 5
      echo "http-server: listening on port ${toString httpPort}"
      exec ${lib.getExe pkgs.python3} -m http.server ${toString httpPort}
    '';
    ports.http.allocate = 18200;
    ready.http.get = {
      port = httpPort;
      path = "/";
    };
    restart.on = "on_failure";
  };

  # 2. Process that depends on the HTTP server being ready
  processes.worker = {
    exec = ''
      mkdir -p ${markerDir}
      touch ${markerDir}/worker.started
      echo "worker: ready and waiting"
      exec sleep infinity
    '';
    after = [ "devenv:processes:http-server" ];
  };

  # 3. Auto start off process: visible in TUI but not launched
  processes.auto-start-off-proc = {
    start.enable = false;
    exec = ''
      mkdir -p ${markerDir}
      touch ${markerDir}/auto-start-off-proc.started
      echo "auto-start-off-proc: this should never appear"
      exec sleep infinity
    '';
  };

  # 4. Simple foreground process (no dependencies)
  processes.simple = {
    exec = ''
      mkdir -p ${markerDir}
      touch ${markerDir}/simple.started
      echo "simple: running in foreground"
      exec sleep infinity
    '';
    restart.on = "never";
  };

  # 5. Process that depends on an auto start off process (should stay waiting)
  # Only enabled during testing since it blocks wait_for_processes in interactive mode
  processes.blocked-by-auto-start-off = lib.mkIf config.devenv.isTesting {
    exec = ''
      mkdir -p ${markerDir}
      touch ${markerDir}/blocked-by-auto-start-off.started
      echo "blocked-by-auto-start-off: this should never appear"
      exec sleep infinity
    '';
    after = [ "devenv:processes:auto-start-off-proc@started" ];
  };

  # --- Dependency chain: third -> second -> first ---

  # 6. First in chain (no dependencies)
  processes.chain-first = {
    exec = ''
      mkdir -p ${markerDir}
      touch ${markerDir}/chain-first.started
      exec sleep infinity
    '';
  };

  # 7. Second depends on first
  processes.chain-second = {
    exec = ''
      mkdir -p ${markerDir}
      touch ${markerDir}/chain-second.started
      exec sleep infinity
    '';
    after = [ "devenv:processes:chain-first@started" ];
  };

  # 8. Third depends on second (transitive chain)
  processes.chain-third = {
    exec = ''
      mkdir -p ${markerDir}
      touch ${markerDir}/chain-third.started
      exec sleep infinity
    '';
    after = [ "devenv:processes:chain-second@started" ];
  };

  # --- Fan-in: consumer depends on both producer-a and producer-b ---

  # 9. Independent producer A
  processes.producer-a = {
    exec = ''
      mkdir -p ${markerDir}
      touch ${markerDir}/producer-a.started
      exec sleep infinity
    '';
  };

  # 10. Independent producer B
  processes.producer-b = {
    exec = ''
      mkdir -p ${markerDir}
      touch ${markerDir}/producer-b.started
      exec sleep infinity
    '';
  };

  # 11. Consumer depends on both producers
  processes.consumer = {
    exec = ''
      mkdir -p ${markerDir}
      # Verify both producers started before us
      if [ ! -f ${markerDir}/producer-a.started ] || [ ! -f ${markerDir}/producer-b.started ]; then
        touch ${markerDir}/consumer.ordering-violation
      fi
      touch ${markerDir}/consumer.started
      exec sleep infinity
    '';
    after = [
      "devenv:processes:producer-a@started"
      "devenv:processes:producer-b@started"
    ];
  };

  # --- @started dependency: fast-follower starts when slow-starter launches ---

  # 12. Depends on http-server@started (not @ready)
  # Should start as soon as http-server is launched, before it becomes ready
  processes.fast-follower = {
    exec = ''
      mkdir -p ${markerDir}
      touch ${markerDir}/fast-follower.started
      exec sleep infinity
    '';
    after = [ "devenv:processes:http-server@started" ];
  };

  enterTest = ''
    wait_for_processes

    echo "--- Enabled processes started ---"
    for name in http-server worker simple; do
      if [ -f ${markerDir}/$name.started ]; then
        echo "PASS: $name started"
      else
        echo "FAIL: $name did not start"
        exit 1
      fi
    done

    echo "--- Auto start off process did not start ---"
    if [ -f ${markerDir}/auto-start-off-proc.started ]; then
      echo "FAIL: auto start off process should not have started"
      exit 1
    else
      echo "PASS: auto start off process did not start"
    fi

    echo "--- Process depending on auto start off process did not start ---"
    if [ -f ${markerDir}/blocked-by-auto-start-off.started ]; then
      echo "FAIL: blocked-by-auto-start-off should not have started"
      exit 1
    else
      echo "PASS: blocked-by-auto-start-off did not start (waiting on auto start off dep)"
    fi

    echo "--- HTTP server is reachable ---"
    response=$(curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:${toString httpPort}/)
    if [ "$response" = "200" ]; then
      echo "PASS: HTTP server responded with 200"
    else
      echo "FAIL: expected 200, got $response"
      exit 1
    fi

    echo "--- Dependency chain tests ---"
    for name in chain-first chain-second chain-third; do
      if [ -f ${markerDir}/$name.started ]; then
        echo "PASS: $name started"
      else
        echo "FAIL: $name did not start"
        exit 1
      fi
    done

    echo "--- Fan-in dependency tests ---"
    for name in producer-a producer-b consumer; do
      if [ -f ${markerDir}/$name.started ]; then
        echo "PASS: $name started"
      else
        echo "FAIL: $name did not start"
        exit 1
      fi
    done

    if [ -f ${markerDir}/consumer.ordering-violation ]; then
      echo "FAIL: consumer started before both producers"
      exit 1
    else
      echo "PASS: consumer started after both producers"
    fi

    echo "--- @started dependency tests ---"
    if [ -f ${markerDir}/fast-follower.started ]; then
      echo "PASS: fast-follower started"
    else
      echo "FAIL: fast-follower did not start"
      exit 1
    fi

    echo "All process tests passed!"
  '';
}
