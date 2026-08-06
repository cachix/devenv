#!/bin/sh
# Print the PID of the test HTTP server on $1, or nothing when it is absent.
needle="python3 -u -m http.server $1"
ps -eo pid=,comm=,args= |
  awk -v needle="$needle" '
    $2 !~ /(^|\/)awk$/ && index($0, needle) {
      print $1
      exit
    }
  '
