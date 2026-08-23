set -e

. "$DEVENV_TEST_LIB"

WORDPRESS_HTTP_PORT=${WORDPRESS_HTTP_PORT:?WORDPRESS_HTTP_PORT is not set}
WORDPRESS_URL="http://127.0.0.1:$WORDPRESS_HTTP_PORT/index.php"

# Verify PHP extensions are loaded (regression test for #2404)
echo "Checking PHP extensions..."
php_modules=$(php -m)
for ext in mysqli pdo_mysql gd zip intl exif; do
    if ! echo "$php_modules" | grep -qi "^$ext$"; then
        echo "ERROR: PHP extension '$ext' is not loaded"
        echo "Loaded modules:"
        echo "$php_modules"
        exit 1
    fi
done
echo "All required PHP extensions are loaded"

# Wait for the whole process graph (mysql ready + devenv:mysql:configure seeded
# + caddy up) to reach a healthy state.
wait_for_processes

# Verify database exists and the seeded wordpress user can connect
mysql -h 127.0.0.1 -uwordpress -pwordpress wordpress -e 'SELECT 1'

# Test PHP through Caddy. The process readiness probe exercises this same
# endpoint, while the retry keeps the assertion resilient to a restart between
# readiness and the test request.
response_file=wordpress-response.txt
http_status=000

wordpress_serves_ok() {
    http_status=$(curl -sS -o "$response_file" -w '%{http_code}' "$WORDPRESS_URL") || true
    [ "$http_status" = "200" ] && grep -qx 'OK' "$response_file"
}

if wait_until 10 wordpress_serves_ok; then
    echo "WordPress stack test passed"
    exit 0
fi

echo "PHP test failed with HTTP $http_status" >&2
cat "$response_file" >&2 || true
for log in "$DEVENV_RUNTIME"/processes/logs/*; do
    if [ -f "$log" ]; then
        echo "==> $log <==" >&2
        tail -n 80 "$log" >&2
    fi
done
exit 1
