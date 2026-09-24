set -e

wait_for_port "$MYSQL_TCP_PORT"

# Wait for configure-mysql to finish.
sleep 5

# through unix_socket
mysql -e 'SELECT VERSION()'

# through tcp/ip
mysql -h "127.0.0.1" -P "$MYSQL_TCP_PORT" -udb -pdb -e 'SELECT VERSION()'

ping-mysql
