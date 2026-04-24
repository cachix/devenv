# WordPress

This guide sets up a local WordPress development environment with:

- **Caddy** - Web server that handles HTTP requests and routes them to PHP
- **PHP-FPM** - FastCGI Process Manager that executes WordPress PHP code
- **MariaDB** - Database server for storing WordPress content, users, and settings
- **wp-cli** - Command-line interface for managing WordPress

## Quick Start

1. Add the configuration below to your `devenv.nix`
2. Run `devenv up` — services start, the database is seeded, WordPress is downloaded, and `wp-config.php` is created automatically
3. Visit [http://localhost:8000](http://localhost:8000) to complete the WordPress installation

## Configuration

```nix title="devenv.nix"
{ pkgs, config, ... }:

{
  # WordPress CLI for managing WordPress from the command line
  packages = [
    pkgs.wp-cli
  ];

  languages.php = {
    enable = true;
    version = "8.4";

    # PHP extensions required by WordPress
    # Note: common extensions like xml, mbstring, curl are enabled by default
    extensions = [
      "mysqli"    # MySQL database connectivity
      "pdo_mysql" # PDO MySQL driver (used by some plugins)
      "gd"        # Image manipulation (thumbnails, image editing)
      "zip"       # Plugin/theme installation from zip files
      "intl"      # Internationalization support
      "exif"      # Image metadata reading
    ];

    # PHP settings for WordPress
    ini = ''
      memory_limit = 256M
      upload_max_filesize = 64M
      post_max_size = 64M
      max_execution_time = 300
    '';

    # PHP-FPM pool configuration
    # FPM (FastCGI Process Manager) manages PHP worker processes
    fpm.pools.web = {
      settings = {
        "pm" = "dynamic";              # Dynamic process management
        "pm.max_children" = 10;        # Maximum worker processes
        "pm.start_servers" = 2;        # Workers to start initially
        "pm.min_spare_servers" = 1;    # Minimum idle workers
        "pm.max_spare_servers" = 5;    # Maximum idle workers
      };
    };
  };

  # MariaDB database server
  services.mysql = {
    enable = true;
    package = pkgs.mariadb;

    # Create the WordPress database on first run
    initialDatabases = [{ name = "wordpress"; }];

    # Create database user with access to WordPress database
    ensureUsers = [{
      name = "wordpress";
      password = "wordpress";
      ensurePermissions = { "wordpress.*" = "ALL PRIVILEGES"; };
    }];
  };

  # Caddy web server
  services.caddy = {
    enable = true;

    # Serve WordPress on http://localhost:8000
    virtualHosts."http://localhost:8000" = {
      extraConfig = ''
        root * ${config.devenv.root}/wordpress

        # Pass PHP requests to PHP-FPM.
        php_fastcgi unix/${config.languages.php.fpm.pools.web.socket}

        # Serve static files directly
        file_server
      '';
    };
  };

  # Download WordPress and write wp-config.php once MariaDB is ready and seeded.
  # Runs automatically as part of `devenv up` via the caddy process dependency.
  tasks."wordpress:setup" = {
    description = "Download WordPress and create wp-config.php";
    after = [ "devenv:mysql:configure" ];
    cwd = config.devenv.root;
    exec = ''
      set -e

      mkdir -p wordpress
      cd wordpress

      if [ ! -f wp-includes/version.php ]; then
        echo "Downloading WordPress..."
        wp core download
      else
        echo "WordPress already downloaded."
      fi

      if [ ! -f wp-config.php ]; then
        echo "Creating wp-config.php..."
        wp config create \
          --dbname=wordpress \
          --dbuser=wordpress \
          --dbpass=wordpress \
          --dbhost=127.0.0.1
        echo ""
        echo "WordPress configured! Visit http://localhost:8000 to complete installation."
      else
        echo "wp-config.php already exists."
      fi
    '';
  };

  # Hold caddy until WordPress is on disk so the first request isn't a 404.
  processes.caddy.after = [ "wordpress:setup" ];

  # Show helpful instructions when entering the shell
  enterShell = ''
    echo ""
    echo "WordPress Development Environment"
    echo "=================================="
    echo ""
    echo "Run devenv up to start services and provision WordPress, then open:"
    echo "  http://localhost:8000"
    echo ""
    echo "Database credentials (for wp-config.php):"
    echo "  Host:     127.0.0.1"
    echo "  Database: wordpress"
    echo "  User:     wordpress"
    echo "  Password: wordpress"
    echo ""
  '';
}
```

## How It Works

When you run `devenv up`, devenv runs the following graph in order:

1. **MariaDB** starts. `services.mysql` advertises a readiness probe (`mysqladmin ping`), so downstream tasks wait until the server is actually accepting connections, not just until the port is bound.
2. **`devenv:mysql:configure`** runs as a oneshot task once MariaDB is ready, creating the `wordpress` database, the `wordpress` user, and granting privileges.
3. **`wordpress:setup`** runs once `devenv:mysql:configure` has succeeded, downloading WordPress core and writing `wp-config.php`.
4. **Caddy** starts only after `wordpress:setup` completes, so the first HTTP request doesn't hit an empty document root.
5. **PHP-FPM** exposes a Unix socket that Caddy proxies `.php` requests to. The `php_fastcgi` directive handles WordPress pretty permalinks automatically — non-existent paths fall through to `index.php`.

## Troubleshooting

### Database connection errors

`devenv up` seeds the database automatically. If you see "Error establishing database connection", verify the database and user exist:

```bash
mysql -u wordpress -pwordpress -h 127.0.0.1 -e "SHOW DATABASES;"
```

If the `wordpress` user is missing, the `devenv:mysql:configure` task didn't run — check the logs with `devenv tasks list` and `devenv tasks run devenv:mysql:configure`. A stale `.devenv/state/mysql` from an older setup can also cause this; remove it and re-run `devenv up`.

### Port 8000 already in use

If another service is using port 8000, change the port in the Caddy configuration:

```nix
services.caddy.virtualHosts."http://localhost:8080" = { ... };
```

### PHP extension errors

If WordPress reports missing extensions, add them to the `extensions` list:

```nix
languages.php.extensions = [
  "mysqli"
  "imagick"  # Add additional extensions as needed
];
```

## Advanced Configuration

### Adding Redis for caching

Redis improves WordPress performance by caching database queries:

```nix
services.redis.enable = true;

languages.php.extensions = [
  # ... other extensions ...
  "redis"
];
```

Install a Redis object cache plugin (like "Redis Object Cache") in WordPress.

### Adding Xdebug for debugging

Enable step-through debugging in your IDE:

```nix
languages.php.extensions = [
  # ... other extensions ...
  "xdebug"
];

languages.php.ini = ''
  memory_limit = 256M
  xdebug.mode = debug
  xdebug.start_with_request = yes
  xdebug.client_port = 9003
'';
```

### HTTPS with local certificates

For plugins that require HTTPS, use local certificates:

```nix
certificates = [ "localhost" ];

services.caddy.virtualHosts."https://localhost" = {
  extraConfig = ''
    tls ${config.env.DEVENV_STATE}/mkcert/localhost.pem ${config.env.DEVENV_STATE}/mkcert/localhost-key.pem
    root * ${config.devenv.root}/wordpress
    php_fastcgi unix/${config.languages.php.fpm.pools.web.socket}
    file_server
  '';
};
```

Note: HTTPS on port 443 requires elevated privileges. Use a high port like 8443 or configure your system to allow binding to privileged ports.
