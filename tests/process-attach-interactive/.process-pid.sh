#!/bin/sh
# Print the PID of the test HTTP server on $1, or nothing when it is absent.
needle="python3 -u -m http.server $1"
ps -eo pid=,args= |
  awk -v needle="$needle" '
    index($0, needle) && !index($0, "awk -v needle=") {
      print $1
      exit
    }
  '
