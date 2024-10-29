python - <<EOF
import os

assert "LD_LIBRARY_PATH" not in os.environ
EOF


LD_LIBRARY_PATH=set-from-shell python - <<EOF
import os
import sys
print(sys.prefix)
assert os.environ["LD_LIBRARY_PATH"] == "set-from-shell"
EOF
