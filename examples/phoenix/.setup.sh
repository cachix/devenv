set +x

mix local.hex --force
mix local.rebar --force
mix archive.install --force hex phx_new

echo y | mix phx.new --install hello
sed -i.bak -e "s/hostname: \"localhost\"/socket_dir: System.get_env(\"PGHOST\")/" \
  ./hello/config/dev.exs && rm ./hello/config/dev.exs.bak
