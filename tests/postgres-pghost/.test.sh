wait_for_port 2345
psql postgres -c '\q' &> /dev/null

# Check the exit status of the psql command
if [ $? -eq 0 ]; then
    echo "listen_address and PGHOST is valid, connection successful"
    exit 0
else
    echo "listen_address and PGHOST is invalid, connection failed"
    exit 1
fi