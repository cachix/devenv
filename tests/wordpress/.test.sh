set -e

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

# Wait for MySQL
wait_for_port 3306

# Wait for configure-mysql to finish
sleep 5

# Verify database exists and user can connect
mysql -h 127.0.0.1 -uwordpress -pwordpress wordpress -e 'SELECT 1'

# Wait for Caddy
wait_for_port 8000

# Create a test PHP file
cat > index.php << 'PHPEOF'
<?php
// Test database connection
$conn = new mysqli('127.0.0.1', 'wordpress', 'wordpress', 'wordpress');
if ($conn->connect_error) {
    http_response_code(500);
    die("DB error: " . $conn->connect_error);
}
$conn->close();
echo "OK";
PHPEOF

# Test PHP through Caddy
response=$(curl -sf http://localhost:8000/index.php)
if [ "$response" != "OK" ]; then
    echo "PHP test failed: $response"
    exit 1
fi

echo "WordPress stack test passed"
