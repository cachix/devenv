prev=$(which bash)
cat << EOF >> bash
PS1="$ " $prev --norc --noprofile
EOF
chmod +x bash
export PATH=$PWD:$PATH