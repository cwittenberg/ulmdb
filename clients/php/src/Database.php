<?php
declare(strict_types=1);

namespace Ulmdb;

use FFI;

/** Transaction-bound database. All returned strings own their bytes. */
final class Database
{
    use NativeObject;

    /** @internal Use Transaction::openDatabase(). */
    public function __construct(private readonly Transaction $transaction, private readonly int $dbi)
    {
    }

    public function get(string $key): ?string
    {
        [$native, $handle] = $this->transaction->binding();
        [$keyValue, $backing] = $native->input($key);
        $value = $native->ffi->new('MDB_val');
        $code = $native->ffi->mdb_get($handle, $this->dbi, FFI::addr($keyValue), FFI::addr($value));
        if ($code === Native::NOTFOUND) {
            return null;
        }
        $native->check($code);
        return Native::output($value);
    }

    public function put(string $key, string $value, bool $overwrite = true, bool $noDuplicateData = false): void
    {
        [$native, $handle] = $this->transaction->binding(write: true);
        [$keyValue, $keyBacking] = $native->input($key);
        [$dataValue, $dataBacking] = $native->input($value);
        $native->check($native->ffi->mdb_put(
            $handle, $this->dbi, FFI::addr($keyValue), FFI::addr($dataValue),
            ($overwrite ? 0 : 0x10) | ($noDuplicateData ? 0x20 : 0)
        ));
    }

    public function delete(string $key, ?string $value = null): bool
    {
        [$native, $handle] = $this->transaction->binding(write: true);
        [$keyValue, $keyBacking] = $native->input($key);
        [$dataValue, $dataBacking] = $value === null ? [null, null] : $native->input($value);
        $code = $native->ffi->mdb_del(
            $handle, $this->dbi, FFI::addr($keyValue), $dataValue === null ? null : FFI::addr($dataValue)
        );
        if ($code === Native::NOTFOUND) {
            return false;
        }
        $native->check($code);
        return true;
    }

    /** @return list<array{string, string}> All ordered pairs, including duplicates; eagerly copied. */
    public function entries(bool $reverse = false): array
    {
        [$native, $handle] = $this->transaction->binding();
        $cursor = $native->ffi->new('MDB_cursor *');
        $native->check($native->ffi->mdb_cursor_open($handle, $this->dbi, FFI::addr($cursor)));
        try {
            $key = $native->ffi->new('MDB_val');
            $value = $native->ffi->new('MDB_val');
            $operation = $reverse ? 6 : 0; // MDB_LAST / MDB_FIRST
            $pairs = [];
            while (true) {
                $code = $native->ffi->mdb_cursor_get($cursor, FFI::addr($key), FFI::addr($value), $operation);
                if ($code === Native::NOTFOUND) {
                    return $pairs;
                }
                $native->check($code);
                $pairs[] = [Native::output($key), Native::output($value)];
                $operation = $reverse ? 12 : 8; // MDB_PREV / MDB_NEXT
            }
        } finally {
            $native->ffi->mdb_cursor_close($cursor);
        }
    }

    public function putPrefix(string $cidr, string $value): void
    {
        [$native, $handle] = $this->transaction->binding(write: true);
        [$dataValue, $backing] = $native->input($value);
        $native->check($native->ffi->ulmdb_prefix_put(
            $handle, $this->dbi, Native::text($cidr), FFI::addr($dataValue)
        ));
    }

    public function getPrefix(string $cidr): ?string
    {
        [$native, $handle] = $this->transaction->binding();
        $value = $native->ffi->new('MDB_val');
        $code = $native->ffi->ulmdb_prefix_get($handle, $this->dbi, Native::text($cidr), FFI::addr($value));
        if ($code === Native::NOTFOUND) {
            return null;
        }
        $native->check($code);
        return Native::output($value);
    }

    public function deletePrefix(string $cidr): bool
    {
        [$native, $handle] = $this->transaction->binding(write: true);
        $code = $native->ffi->ulmdb_prefix_delete($handle, $this->dbi, Native::text($cidr));
        if ($code === Native::NOTFOUND) {
            return false;
        }
        $native->check($code);
        return true;
    }

    /** @return array{prefix: string, value: string}|null */
    public function longestMatch(string $address): ?array
    {
        [$native, $handle] = $this->transaction->binding();
        $prefix = $native->ffi->new('MDB_val');
        $value = $native->ffi->new('MDB_val');
        $code = $native->ffi->ulmdb_longest_match(
            $handle, $this->dbi, Native::text($address), FFI::addr($prefix), FFI::addr($value)
        );
        if ($code === Native::NOTFOUND) {
            return null;
        }
        $native->check($code);
        return ['prefix' => Native::output($prefix), 'value' => Native::output($value)];
    }
}
