<?php
declare(strict_types=1);

require dirname(__DIR__) . '/autoload.php';

use Ulmdb\Environment;
use Ulmdb\Transaction;

function same(mixed $actual, mixed $expected): void
{
    if ($actual !== $expected) {
        throw new RuntimeException('PHP interoperability mismatch: ' . var_export([$actual, $expected], true));
    }
}

if ($argc !== 2) {
    throw new InvalidArgumentException('Usage: php interoperability.php DATABASE');
}

$environment = new Environment($argv[1]);
try {
    $environment->read(function (Transaction $transaction): void {
        $kv = $transaction->openDatabase('interop-kv');
        same($kv->get("\0python\xff"), "python\0value\xff");
        same($kv->get('empty'), '');
        same($kv->get('missing'), null);
        same($transaction->openDatabase('interop-duplicates', kind: 'duplicates')->entries(), [['key', 'a'], ['key', 'z']]);
        same($transaction->openDatabase('interop-v4', kind: 'ipv4')->longestMatch('192.0.2.200'), ['prefix' => '192.0.2.0/24', 'value' => 'python-v4']);
        same($transaction->openDatabase('interop-v6', kind: 'ipv6')->longestMatch('2001:db8:1::1'), ['prefix' => '2001:db8::/32', 'value' => 'python-v6']);
    });
    $environment->write(function (Transaction $transaction): void {
        $transaction->openDatabase('interop-kv')->put("\0php\xfe", "php\0value\xfe");
        $transaction->openDatabase('interop-duplicates', kind: 'duplicates')->put('key', 'm');
        $transaction->openDatabase('interop-v4', kind: 'ipv4')->putPrefix('192.0.2.128/25', 'php-v4');
        $transaction->openDatabase('interop-v6', kind: 'ipv6')->putPrefix('2001:db8:1::/48', 'php-v6');
    });
} finally {
    $environment->close();
}
echo "PHP read Python records and committed binary KV, duplicates and IPv4/IPv6 prefixes\n";
