<?php
declare(strict_types=1);

// Source-checkout loader. Installed applications can use Composer's autoloader.
spl_autoload_register(static function (string $class): void {
    $prefix = 'Ulmdb\\';
    if (strncmp($class, $prefix, strlen($prefix)) !== 0) {
        return;
    }
    $file = __DIR__ . '/src/' . str_replace('\\', '/', substr($class, strlen($prefix))) . '.php';
    if (is_file($file)) {
        require $file;
    }
});
