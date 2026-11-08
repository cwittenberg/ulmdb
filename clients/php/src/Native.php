<?php
declare(strict_types=1);

namespace Ulmdb;

use FFI;
use FFI\CData;

/** @internal Small, private-to-the-client C ABI binding. */
final class Native
{
    public const NOTFOUND = -30798;
    public const RDONLY = 0x20000;
    public const NOTLS = 0x200000;
    public const CREATE = 0x40000;

    public readonly FFI $ffi;

    private const DECLARATIONS = <<<'C'
typedef struct MDB_env MDB_env;
typedef struct MDB_txn MDB_txn;
typedef struct MDB_cursor MDB_cursor;
typedef unsigned int MDB_dbi;
typedef struct MDB_val { size_t mv_size; void *mv_data; } MDB_val;
char *mdb_strerror(int);
int mdb_env_create(MDB_env **);
int mdb_env_set_maxdbs(MDB_env *, MDB_dbi);
int mdb_env_open(MDB_env *, const char *, unsigned int, unsigned int);
void mdb_env_close(MDB_env *);
int mdb_txn_begin(MDB_env *, MDB_txn *, unsigned int, MDB_txn **);
int mdb_txn_commit(MDB_txn *);
void mdb_txn_abort(MDB_txn *);
int ulmdb_dbi_open(MDB_txn *, const char *, unsigned int, unsigned int, MDB_dbi *);
int mdb_get(MDB_txn *, MDB_dbi, MDB_val *, MDB_val *);
int mdb_put(MDB_txn *, MDB_dbi, MDB_val *, MDB_val *, unsigned int);
int mdb_del(MDB_txn *, MDB_dbi, MDB_val *, MDB_val *);
int mdb_cursor_open(MDB_txn *, MDB_dbi, MDB_cursor **);
int mdb_cursor_get(MDB_cursor *, MDB_val *, MDB_val *, int);
void mdb_cursor_close(MDB_cursor *);
int ulmdb_prefix_put(MDB_txn *, MDB_dbi, const char *, const MDB_val *);
int ulmdb_prefix_get(MDB_txn *, MDB_dbi, const char *, MDB_val *);
int ulmdb_prefix_delete(MDB_txn *, MDB_dbi, const char *);
int ulmdb_longest_match(MDB_txn *, MDB_dbi, const char *, MDB_val *, MDB_val *);
C;

    public function __construct(?string $library = null)
    {
        if (!extension_loaded('ffi')) {
            throw new \RuntimeException('ULMDB requires the PHP FFI extension');
        }
        $configured = $library ?? (getenv('ULMDB_LIBRARY') ?: null);
        if ($configured !== null) {
            self::text($configured);
            $candidates = [$configured];
        } else {
            $root = dirname(__DIR__, 3);
            $name = PHP_OS_FAMILY === 'Darwin' ? 'libulmdb_c.dylib' : 'libulmdb_c.so';
            $candidates = [$root . '/target/debug/' . $name, $root . '/target/release/' . $name, $name];
        }
        $last = null;
        foreach ($candidates as $candidate) {
            try {
                $this->ffi = FFI::cdef(self::DECLARATIONS, $candidate);
                return;
            } catch (\Throwable $error) {
                $last = $error;
            }
        }
        throw new \RuntimeException(
            'Cannot load ULMDB. Set ULMDB_LIBRARY to libulmdb_c and enable FFI (php -d ffi.enable=true). '
            . ($last?->getMessage() ?? ''), 0, $last
        );
    }

    public function check(int $code): void
    {
        if ($code !== 0) {
            throw new Error(FFI::string($this->ffi->mdb_strerror($code)), $code);
        }
    }

    public static function text(string $text): string
    {
        if (str_contains($text, "\0")) {
            throw new \InvalidArgumentException('embedded NUL is not allowed in paths, names or addresses');
        }
        return $text;
    }

    /** @return array{CData, CData} Keep both the value and its backing alive during the C call. */
    public function input(string $bytes): array
    {
        $size = strlen($bytes);
        $backing = $this->ffi->new('char[' . ($size + 1) . ']');
        if ($size > 0) {
            FFI::memcpy($backing, $bytes, $size);
        }
        $value = $this->ffi->new('MDB_val');
        $value->mv_size = $size;
        $value->mv_data = FFI::addr($backing[0]);
        return [$value, $backing];
    }

    public static function output(CData $value): string
    {
        return $value->mv_size === 0 ? '' : FFI::string($value->mv_data, $value->mv_size);
    }
}
