set -e

# The shell's PHP loads the extension.
php -r 'exit(extension_loaded("shmop") ? 0 : 1);'

# A rebuilt variant of the same package is thread-safe and loads its own build of the extension.
"$REBUILT_PHP" -r 'if (!PHP_ZTS) { fwrite(STDERR, "not thread-safe\n"); exit(1); } exit(extension_loaded("shmop") ? 0 : 1);'
