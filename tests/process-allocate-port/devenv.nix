{ config, lib, pkgs, ... }:

{
  packages = [ pkgs.curl ];

  processes.server1 = {
    ports.http.allocate = 18080;
    exec = ''
      ${lib.getExe pkgs.python3} -m http.server ${toString config.processes.server1.ports.http.value}
    '';
  };

  processes.server2 = {
    ports.http.allocate = 18080;
    exec = ''
      ${lib.getExe pkgs.python3} -m http.server ${toString config.processes.server2.ports.http.value}
    '';
  };

  enterTest =
    let
      port1 = toString config.processes.server1.ports.http.value;
      port2 = toString config.processes.server2.ports.http.value;
    in
    ''
      # Verify ports are distinct
      if [ "${port1}" = "${port2}" ]; then
        echo "✗ Ports should be distinct but both are ${port1}"
        exit 1
      fi
      echo "✓ Ports are distinct: ${port1} and ${port2}"

      # Wait for both servers to be ready
      wait_for_port ${port1}
      echo "✓ Port ${port1} is open"
      wait_for_port ${port2}
      echo "✓ Port ${port2} is open"

      # Verify both servers respond
      response1=$(curl -s -o /dev/null -w "%{http_code}" http://localhost:${port1}/)
      response2=$(curl -s -o /dev/null -w "%{http_code}" http://localhost:${port2}/)

      if [ "$response1" = "200" ] && [ "$response2" = "200" ]; then
        echo "✓ Both HTTP servers responded with 200"
      else
        echo "✗ Expected HTTP 200 from both servers, got $response1 and $response2"
        exit 1
      fi

      # Test --strict-ports flag: should fail when port is already in use
      echo "Testing --strict-ports flag..."

      # server1 is already running on port1, so strict mode should fail
      # when trying to allocate that same port
      if output=$(devenv up --strict-ports 2>&1); then
        echo "✗ Expected devenv up --strict-ports to fail (port ${port1} is in use)"
        exit 1
      fi

      if echo "$output" | grep -q "already in use"; then
        echo "✓ --strict-ports correctly failed with port conflict error"
      else
        echo "✗ Expected error message to contain 'already in use', got: $output"
        exit 1
      fi
    '';
}
