{ config, ... }:

let
  outputPath = "${config.devenv.state}/output.txt";
in
{
  processes.enabled-proc = {
    exec = ''
      echo "enabled started" >> ${outputPath}
      echo first line
      sleep 60
    '';
  };

  processes.disabled-proc = {
    start.enable = false;
    exec = ''
      echo "disabled started" >> ${outputPath}
      echo first line
      sleep 60
    '';
  };

  enterTest = ''
    wait_for_processes

    if [ ! -f ${outputPath} ]; then
      echo "FAIL: output file was not created"
      exit 1
    fi

    if grep -q "enabled started" ${outputPath}; then
      echo "PASS: enabled process started"
    else
      echo "FAIL: enabled process did not start"
      exit 1
    fi

    if grep -q "disabled started" ${outputPath}; then
      echo "FAIL: disabled process should not have started"
      exit 1
    else
      echo "PASS: disabled process did not start"
    fi
  '';
}
