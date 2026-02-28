{ config, lib, pkgs, ... }:

let
  outputPath = "${config.devenv.state}/output.txt";
  httpPort = config.processes.http-server.ports.http.value;
in
{
  packages = [ pkgs.curl ];

  # 1. Enabled process with readiness probe and port allocation
  processes.http-server = {
    exec = ''
      echo "http-server started" >> ${outputPath}
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
      echo "worker started" >> ${outputPath}
      echo "worker: ready and waiting"
      exec sleep infinity
    '';
    after = [ "devenv:processes:http-server" ];
  };

  # 3. Disabled process: visible in TUI but not launched
  processes.disabled-proc = {
    start.enable = false;
    exec = ''
      echo "disabled started" >> ${outputPath}
      echo "disabled-proc: this should never appear"
      exec sleep infinity
    '';
  };

  # 4. Simple foreground process (no readiness probe, no restart)
  processes.simple = {
    exec = ''
      echo "simple started" >> ${outputPath}
      echo "simple: running in foreground"
      exec sleep infinity
    '';
    restart.on = "never";
  };

  enterTest = ''
    wait_for_processes

    echo "--- Checking output file ---"
    if [ ! -f ${outputPath} ]; then
      echo "FAIL: output file was not created"
      exit 1
    fi
    cat ${outputPath}

    echo "--- Enabled processes started ---"
    for name in http-server worker simple; do
      if grep -q "$name started" ${outputPath}; then
        echo "PASS: $name started"
      else
        echo "FAIL: $name did not start"
        exit 1
      fi
    done

    echo "--- Disabled process did not start ---"
    if grep -q "disabled started" ${outputPath}; then
      echo "FAIL: disabled process should not have started"
      exit 1
    else
      echo "PASS: disabled process did not start"
    fi

    echo "--- HTTP server is reachable ---"
    response=$(curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:${toString httpPort}/)
    if [ "$response" = "200" ]; then
      echo "PASS: HTTP server responded with 200"
    else
      echo "FAIL: expected 200, got $response"
      exit 1
    fi

    echo "--- Worker started after HTTP server ---"
    http_line=$(grep -n "http-server started" ${outputPath} | head -1 | cut -d: -f1)
    worker_line=$(grep -n "worker started" ${outputPath} | head -1 | cut -d: -f1)
    if [ "$http_line" -le "$worker_line" ]; then
      echo "PASS: http-server (line $http_line) started before worker (line $worker_line)"
    else
      echo "FAIL: worker started before http-server"
      exit 1
    fi

    echo "All process tests passed!"
  '';
}
