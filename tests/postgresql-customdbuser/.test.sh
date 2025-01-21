set -e

wait_for_port 2345
pg_isready -d template1

# negative check (whether error handling in the test is reliable)
psql \
	--set ON_ERROR_STOP=on \
	--username=notexists \
	--dbname=testdb \
	--echo-all \
	-c '\dt' && {
	echo "Problem with error handling!!!"
	exit 1
}

# now check whether we can connect to our db as our new user and have permission to do stuff with the DB
psql \
	--set ON_ERROR_STOP=on \
	--username=testuser \
	--dbname=testdb \
	--echo-all \
	--file=- <<'EOF'
\dt
SELECT * FROM supermasters;
INSERT INTO 
    supermasters (ip,nameserver,account)
    VALUES ('10.100.9.99','dns.example.org','exampleaccount');
SELECT * FROM supermasters;
EOF
