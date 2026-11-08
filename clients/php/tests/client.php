<?php
declare(strict_types=1);

require dirname(__DIR__) . '/autoload.php';

use Ulmdb\Environment;
use Ulmdb\Error as DatabaseError;
use Ulmdb\Transaction;

/** Assertions stay active even when PHP's built-in assert() is disabled. */
function same(mixed $expected, mixed $actual): void
{
    if ($expected !== $actual) {
        throw new RuntimeException('Expected ' . var_export($expected, true)
            . ', got ' . var_export($actual, true));
    }
}

function raises(string $class, callable $operation, ?int $code = null): Throwable
{
    try {
        $operation();
    } catch (Throwable $error) {
        if (!$error instanceof $class) {
            throw new RuntimeException("Expected {$class}, got " . get_class($error), 0, $error);
        }
        if ($code !== null) {
            same($code, $error->getCode());
        }
        return $error;
    }
    throw new RuntimeException("Expected {$class}, but no exception was thrown");
}

function removeTree(string $path): void
{
    if (is_link($path) || is_file($path)) {
        unlink($path);
        return;
    }
    if (!is_dir($path)) {
        return;
    }
    foreach (new DirectoryIterator($path) as $entry) {
        if (!$entry->isDot()) {
            removeTree($entry->getPathname());
        }
    }
    rmdir($path);
}

function fixture(callable $test): void
{
    $root = sys_get_temp_dir() . '/ulmdb-php-' . bin2hex(random_bytes(12));
    if (!mkdir($root, 0700)) {
        throw new RuntimeException('Cannot create temporary test directory');
    }
    $environment = null;
    try {
        $environment = new Environment($root . '/database');
        $test($environment, $root . '/database', $root);
    } finally {
        if ($environment !== null) {
            $environment->close();
        }
        removeTree($root);
    }
}

$tests = [];

$tests['binary and empty values persist after reopen'] = static function (): void {
    fixture(static function (Environment $env, string $path): void {
        $env->write(static function (Transaction $tx): void {
            $db = $tx->openDatabase('bytes', create: true);
            $db->put("a\0b", "\0\xffvalue\0");
            $db->put('empty', '');
            same("\0\xffvalue\0", $db->get("a\0b"));
            same('', $db->get('empty'));
            same(null, $db->get('absent'));
        });
        $env->close();
        $reopened = new Environment($path);
        try {
            $copy = $reopened->read(static function (Transaction $tx): string {
                $db = $tx->openDatabase('bytes');
                same('', $db->get('empty'));
                return $db->get("a\0b");
            });
        } finally {
            $reopened->close();
        }
        same("\0\xffvalue\0", $copy);
    });
};

$tests['unnamed database and callback return values'] = static function (): void {
    fixture(static function (Environment $env): void {
        same(42, $env->write(static function (Transaction $tx): int {
            $tx->openDatabase()->put('key', 'value');
            return 42;
        }));
        same('value', $env->read(static fn (Transaction $tx): ?string =>
            $tx->openDatabase()->get('key')));
    });
};

$tests['explicit commit and idempotent abort'] = static function (): void {
    fixture(static function (Environment $env): void {
        $tx = $env->begin(write: true);
        $db = $tx->openDatabase('kv', create: true);
        $db->put('key', 'original');
        $tx->commit();
        raises(LogicException::class, static fn () => $tx->commit());
        raises(LogicException::class, static fn () => $db->get('key'));

        $tx = $env->begin(write: true);
        $tx->openDatabase('kv')->put('key', 'aborted');
        $tx->abort();
        $tx->abort();
        same('original', $env->read(static fn (Transaction $tx): ?string =>
            $tx->openDatabase('kv')->get('key')));
    });
};

$tests['callback exception rolls back and releases writer'] = static function (): void {
    fixture(static function (Environment $env): void {
        $env->write(static fn (Transaction $tx) =>
            $tx->openDatabase('kv', create: true)->put('key', 'original'));
        $failure = new RuntimeException('rollback');
        $caught = raises(RuntimeException::class, static function () use ($env, $failure): void {
            $env->write(static function (Transaction $tx) use ($failure): void {
                $tx->openDatabase('kv')->put('key', 'discarded');
                throw $failure;
            });
        });
        same($failure, $caught);
        same('original', $env->read(static fn (Transaction $tx): ?string =>
            $tx->openDatabase('kv')->get('key')));
        $env->write(static fn (Transaction $tx) =>
            $tx->openDatabase('kv')->put('key', 'next writer'));

        raises(RuntimeException::class, static function () use ($env, $failure): void {
            $env->read(static function (Transaction $tx) use ($failure): void {
                $tx->openDatabase('kv');
                throw $failure;
            });
        });
        $env->close();
    });
};

$tests['lazy generator callbacks are rejected and release transactions'] = static function (): void {
    fixture(static function (Environment $env): void {
        $env->write(static fn (Transaction $tx) =>
            $tx->openDatabase('kv', create: true)->put('key', 'original'));
        raises(LogicException::class, static function () use ($env): void {
            $env->write(static function (Transaction $tx): Generator {
                $tx->openDatabase('kv')->put('key', 'never executed');
                yield 'lazy';
            });
        });
        raises(LogicException::class, static function () use ($env): void {
            $env->write(static function (Transaction $tx): Generator {
                $tx->openDatabase('kv')->put('key', 'discarded');
                return (static function (): Generator { yield 'lazy'; })();
            });
        });
        raises(LogicException::class, static function () use ($env): void {
            $env->read(static function (Transaction $tx): Generator {
                yield $tx->openDatabase('kv')->get('key');
            });
        });
        same('original', $env->read(static fn (Transaction $tx): ?string =>
            $tx->openDatabase('kv')->get('key')));
        $env->write(static fn (Transaction $tx) =>
            $tx->openDatabase('kv')->put('key', 'next writer'));
        $env->close();
    });
};

$tests['aborted database creation stays absent'] = static function (): void {
    fixture(static function (Environment $env): void {
        $tx = $env->begin(write: true);
        $tx->openDatabase('aborted', create: true)->put('key', 'discarded');
        $tx->abort();
        $env->read(static function (Transaction $tx): void {
            raises(DatabaseError::class, static fn () => $tx->openDatabase('aborted'), -30798);
        });
        $env->write(static fn (Transaction $tx) =>
            $tx->openDatabase('aborted', create: true)->put('key', 'committed'));
    });
};

$tests['overlapping snapshots and copied values'] = static function (): void {
    fixture(static function (Environment $env): void {
        $env->write(static fn (Transaction $tx) =>
            $tx->openDatabase('kv', create: true)->put('key', 'before'));
        $before = $env->begin();
        try {
            $db = $before->openDatabase('kv');
            $copied = $db->get('key');
            $env->write(static fn (Transaction $tx) =>
                $tx->openDatabase('kv')->put('key', 'after'));
            same('before', $db->get('key'));
            same('after', $env->read(static fn (Transaction $tx): ?string =>
                $tx->openDatabase('kv')->get('key')));
        } finally {
            $before->abort();
        }
        same('before', $copied);
    });
};

$tests['ordered entries are copied and include binary keys'] = static function (): void {
    fixture(static function (Environment $env): void {
        $env->write(static function (Transaction $tx): void {
            $db = $tx->openDatabase('ordered', create: true);
            foreach ([['z', 'Z'], ["a\0b", "\xff\0"], ['a', ''], ['m', 'M']] as [$key, $value]) {
                $db->put($key, $value);
            }
        });
        $ascending = [['a', ''], ["a\0b", "\xff\0"], ['m', 'M'], ['z', 'Z']];
        $copied = $env->read(static function (Transaction $tx) use ($ascending): array {
            $db = $tx->openDatabase('ordered');
            same($ascending, $db->entries());
            same(array_reverse($ascending), $db->entries(reverse: true));
            return $db->entries();
        });
        $env->close();
        same($ascending, $copied);
    });
};

$tests['overwrite errors and delete presence'] = static function (): void {
    fixture(static function (Environment $env): void {
        $env->write(static function (Transaction $tx): void {
            $db = $tx->openDatabase('kv', create: true);
            $db->put('key', 'value', overwrite: false);
            raises(DatabaseError::class, static fn () =>
                $db->put('key', 'other', overwrite: false), -30799);
            same('value', $db->get('key'));
            same(true, $db->delete('key'));
            same(false, $db->delete('key'));
            same(null, $db->get('key'));
        });
    });
};

$tests['duplicates sort uniquely and delete one pair'] = static function (): void {
    fixture(static function (Environment $env): void {
        $env->write(static function (Transaction $tx): void {
            $db = $tx->openDatabase('dups', kind: 'duplicates', create: true);
            foreach (['z', 'a', 'm', 'a'] as $value) {
                $db->put('key', $value);
            }
            same('a', $db->get('key'));
            $expected = [['key', 'a'], ['key', 'm'], ['key', 'z']];
            same($expected, $db->entries());
            same(array_reverse($expected), $db->entries(reverse: true));
            raises(DatabaseError::class, static fn () =>
                $db->put('key', 'a', noDuplicateData: true), -30799);
            same(true, $db->delete('key', 'm'));
            same(false, $db->delete('key', 'missing'));
            same([['key', 'a'], ['key', 'z']], $db->entries());
            same(true, $db->delete('key'));
            same(null, $db->get('key'));
            same([], $db->entries());
            same([], $db->entries(reverse: true));
        });
    });
};

$tests['native IPv4 and IPv6 exact and longest matches'] = static function (): void {
    fixture(static function (Environment $env): void {
        $env->write(static function (Transaction $tx): void {
            $v4 = $tx->openDatabase('routes4', kind: 'ipv4', create: true);
            $v6 = $tx->openDatabase('routes6', kind: 'ipv6', create: true);
            $v4->putPrefix('10.0.0.0/8', 'private');
            $v4->putPrefix('10.2.0.0/16', "specific\0\xff");
            $v6->putPrefix('2001:db8::/32', 'documentation');
            $v6->putPrefix('2001:db8:1234::/48', 'specific6');
        });
        $copied = $env->read(static function (Transaction $tx): array {
            $v4 = $tx->openDatabase('routes4', kind: 'ipv4');
            $v6 = $tx->openDatabase('routes6', kind: 'ipv6');
            same(['prefix' => '10.2.0.0/16', 'value' => "specific\0\xff"],
                $v4->longestMatch('10.2.3.4'));
            same(['prefix' => '10.0.0.0/8', 'value' => 'private'], $v4->longestMatch('10.3.3.4'));
            same(['prefix' => '2001:db8:1234::/48', 'value' => 'specific6'],
                $v6->longestMatch('2001:db8:1234::1'));
            same('documentation', $v6->getPrefix('2001:db8::/32'));
            same(null, $v4->getPrefix('10.3.0.0/16'));
            same(null, $v4->longestMatch('192.0.2.1'));
            same(null, $v6->longestMatch('2001:db9::1'));
            raises(DatabaseError::class, static fn () => $v4->longestMatch('2001:db8::1'));
            raises(DatabaseError::class, static fn () => $v6->longestMatch('not an IP address'));
            return $v4->longestMatch('10.2.3.4');
        });
        same(['prefix' => '10.2.0.0/16', 'value' => "specific\0\xff"], $copied);
        $env->write(static function (Transaction $tx): void {
            $v4 = $tx->openDatabase('routes4', kind: 'ipv4');
            same(true, $v4->deletePrefix('10.2.0.0/16'));
            same(false, $v4->deletePrefix('10.2.0.0/16'));
            same(['prefix' => '10.0.0.0/8', 'value' => 'private'], $v4->longestMatch('10.2.3.4'));
        });
    });
};

$tests['empty prefix values remain distinct from missing values'] = static function (): void {
    fixture(static function (Environment $env): void {
        $env->write(static function (Transaction $tx): void {
            $db = $tx->openDatabase('routes4', kind: 'ipv4', create: true);
            $db->putPrefix('192.0.2.0/24', '');
            same('', $db->getPrefix('192.0.2.0/24'));
            same(['prefix' => '192.0.2.0/24', 'value' => ''], $db->longestMatch('192.0.2.1'));
        });
    });
};

$tests['read-only transactions and environment reject writes'] = static function (): void {
    fixture(static function (Environment $env, string $path): void {
        $env->write(static function (Transaction $tx): void {
            $tx->openDatabase('kv', create: true)->put('key', 'value');
            $tx->openDatabase('routes4', kind: 'ipv4', create: true)->putPrefix('10.0.0.0/8', 'private');
        });
        $env->read(static function (Transaction $tx): void {
            $db = $tx->openDatabase('kv');
            $prefixes = $tx->openDatabase('routes4', kind: 'ipv4');
            raises(LogicException::class, static fn () => $db->put('key', 'other'));
            raises(LogicException::class, static fn () => $db->delete('key'));
            raises(LogicException::class, static fn () => $tx->openDatabase('other', create: true));
            raises(LogicException::class, static fn () => $prefixes->putPrefix('10.2.0.0/16', 'other'));
            raises(LogicException::class, static fn () => $prefixes->deletePrefix('10.0.0.0/8'));
        });
        $env->close();
        $readonly = new Environment($path, readOnly: true);
        try {
            same('value', $readonly->read(static fn (Transaction $tx): ?string =>
                $tx->openDatabase('kv')->get('key')));
            raises(LogicException::class, static fn () => $readonly->begin(write: true));
            raises(LogicException::class, static fn () => $readonly->write(static fn () => null));
        } finally {
            $readonly->close();
        }
    });
};

$tests['active transaction and closed handle guards'] = static function (): void {
    fixture(static function (Environment $env): void {
        $tx = $env->begin(write: true);
        $db = $tx->openDatabase('kv', create: true);
        raises(LogicException::class, static fn () => $env->close());
        raises(LogicException::class, static fn () => $env->begin(write: true));
        $tx->abort();
        raises(LogicException::class, static fn () => $tx->openDatabase());
        raises(LogicException::class, static fn () => $db->get('key'));
        raises(LogicException::class, static fn () => $db->put('key', 'value'));
        raises(LogicException::class, static fn () => $db->entries());
        $env->close();
        $env->close();
        raises(LogicException::class, static fn () => $env->begin());
    });
};

$tests['invalid names paths kinds and limits fail clearly'] = static function (): void {
    fixture(static function (Environment $env, string $path): void {
        raises(InvalidArgumentException::class, static fn () => new Environment($path . "\0suffix"));
        raises(InvalidArgumentException::class, static fn () => new Environment($path, maxDatabases: 0));
        raises(InvalidArgumentException::class, static fn () => new Environment($path, maxDatabases: 4097));
        $env->write(static function (Transaction $tx): void {
            raises(InvalidArgumentException::class, static fn () => $tx->openDatabase("bad\0name", create: true));
            raises(InvalidArgumentException::class, static fn () => $tx->openDatabase('bad', kind: 'unknown'));
            raises(InvalidArgumentException::class, static fn () => $tx->openDatabase(kind: 'ipv4'));
            $db = $tx->openDatabase('routes4', kind: 'ipv4', create: true);
            raises(InvalidArgumentException::class, static fn () => $db->putPrefix("10.0.0.0/8\0suffix", 'value'));
            raises(InvalidArgumentException::class, static fn () => $db->getPrefix("10.0.0.0/8\0suffix"));
            raises(InvalidArgumentException::class, static fn () => $db->deletePrefix("10.0.0.0/8\0suffix"));
            raises(InvalidArgumentException::class, static fn () => $db->longestMatch("10.0.0.1\0suffix"));
            raises(DatabaseError::class, static fn () => $db->putPrefix('10.0.0.1/8', 'host bits'));
        });
    });
};

$tests['discarded transaction aborts and releases its writer'] = static function (): void {
    fixture(static function (Environment $env): void {
        $tx = $env->begin(write: true);
        $tx->openDatabase('discarded', create: true)->put('key', 'discarded');
        unset($tx);
        gc_collect_cycles();
        $env->write(static fn (Transaction $tx) =>
            $tx->openDatabase('kv', create: true)->put('key', 'new writer'));
        $env->read(static function (Transaction $tx): void {
            raises(DatabaseError::class, static fn () => $tx->openDatabase('discarded'), -30798);
        });
        $env->close();
    });
};

$tests['process shutdown aborts a retained writer without errors'] = static function (): void {
    fixture(static function (Environment $env, string $path): void {
        $env->write(static fn (Transaction $tx) =>
            $tx->openDatabase('kv', create: true)->put('key', 'original'));
        $command = [PHP_BINARY, '-d', 'ffi.enable=true', '-d', 'display_errors=stderr',
            '-d', 'log_errors=0', '-d', 'error_reporting=-1', __DIR__ . '/shutdown.php', $path];
        $process = proc_open($command, [['pipe', 'r'], ['pipe', 'w'], ['pipe', 'w']], $pipes);
        if (!is_resource($process)) {
            throw new RuntimeException('Cannot launch PHP shutdown probe');
        }
        fclose($pipes[0]);
        $stdout = stream_get_contents($pipes[1]);
        $stderr = stream_get_contents($pipes[2]);
        fclose($pipes[1]);
        fclose($pipes[2]);
        $status = proc_close($process);
        if ($status !== 0 || $stdout !== '' || $stderr !== '') {
            throw new RuntimeException("Shutdown probe exit {$status}; stdout: {$stdout}; stderr: {$stderr}");
        }
        same('original', $env->read(static fn (Transaction $tx): ?string =>
            $tx->openDatabase('kv')->get('key')));
        $env->write(static fn (Transaction $tx) =>
            $tx->openDatabase('kv')->put('key', 'writer after shutdown'));
    });
};

$tests['native owners and database wrappers cannot clone or serialize'] = static function (): void {
    fixture(static function (Environment $env): void {
        $tx = $env->begin(write: true);
        try {
            $db = $tx->openDatabase('kv', create: true);
            foreach ([$env, $tx, $db] as $object) {
                raises(Throwable::class, static fn () => clone $object);
                raises(Throwable::class, static fn () => serialize($object));
            }
            $db->put('key', 'still live');
            same('still live', $db->get('key'));
        } finally {
            $tx->abort();
        }
    });
};

$failed = 0;
foreach ($tests as $name => $test) {
    try {
        $test();
        fwrite(STDOUT, "PASS {$name}\n");
    } catch (Throwable $error) {
        $failed++;
        fwrite(STDERR, "FAIL {$name}: {$error}\n");
    }
}
fwrite(STDOUT, count($tests) . ' tests, ' . $failed . " failures\n");
exit($failed === 0 ? 0 : 1);
