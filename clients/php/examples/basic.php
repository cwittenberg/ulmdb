<?php
declare(strict_types=1);

require dirname(__DIR__) . '/autoload.php';

use Ulmdb\Environment;
use Ulmdb\Transaction;

$root = sys_get_temp_dir() . '/ulmdb-php-example-' . bin2hex(random_bytes(12));
mkdir($root, 0700);
$environment = new Environment($root . '/database');
try {
    $environment->write(static function (Transaction $transaction): void {
        $people = $transaction->openDatabase('people', create: true);
        $people->put('alice', 'Alice');
        $routes = $transaction->openDatabase('routes6', kind: 'ipv6', create: true);
        $routes->putPrefix('2001:db8::/32', 'documentation');
        $routes->putPrefix('2001:db8:1234::/48', 'specific network');
    });
    $environment->read(static function (Transaction $transaction): void {
        foreach ($transaction->openDatabase('people')->entries() as [$key, $value]) {
            echo "$key: $value\n";
        }
        $match = $transaction->openDatabase('routes6', kind: 'ipv6')->longestMatch('2001:db8:1234::1');
        if ($match === null || $match['prefix'] !== '2001:db8:1234::/48') {
            throw new RuntimeException('Expected a longest-prefix match');
        }
        echo $match['prefix'] . ': ' . $match['value'] . "\n";
    });
} finally {
    $environment->close();
    $files = new RecursiveIteratorIterator(
        new RecursiveDirectoryIterator($root, FilesystemIterator::SKIP_DOTS),
        RecursiveIteratorIterator::CHILD_FIRST
    );
    foreach ($files as $file) {
        $file->isDir() ? rmdir($file->getPathname()) : unlink($file->getPathname());
    }
    rmdir($root);
}
