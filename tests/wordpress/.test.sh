set -e

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
