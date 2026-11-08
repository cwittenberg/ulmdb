<?php
declare(strict_types=1);

namespace Ulmdb;

use FFI;
use FFI\CData;

/** A snapshot or writer. Database objects expire when their transaction finishes. */
final class Transaction
{
    use NativeObject;

    /** @internal Use Environment::begin(), read() or write(). */
    public function __construct(
        private readonly Environment $environment,
        private readonly Native $native,
        private ?CData $handle,
        private readonly bool $write
    ) {
    }

    /** @internal Obtain a checked capability for one synchronous operation. */
    public function binding(bool $write = false): array
    {
        if ($this->handle === null) {
            throw new \LogicException('transaction is closed');
        }
        if ($write && !$this->write) {
            throw new \LogicException('transaction is read-only');
        }
        return [$this->native, $this->handle];
    }

    public function openDatabase(?string $name = null, string $kind = 'kv', bool $create = false): Database
    {
        [$native, $handle] = $this->binding($create);
        $kinds = ['kv' => 0, 'duplicates' => 1, 'ipv4' => 4, 'ipv6' => 6];
        if (!isset($kinds[$kind]) || ($name === null && $kind !== 'kv')) {
            throw new \InvalidArgumentException('kind must be kv, duplicates, ipv4 or ipv6; the unnamed database is kv');
        }
        $dbi = $native->ffi->new('MDB_dbi');
        $native->check($native->ffi->ulmdb_dbi_open(
            $handle, $name === null ? null : Native::text($name), $kinds[$kind],
            $create ? Native::CREATE : 0, FFI::addr($dbi)
        ));
        return new Database($this, $dbi->cdata);
    }

    public function commit(): void
    {
        [, $handle] = $this->binding();
        // Commit consumes the handle, including native persistence failures.
        $this->handle = null;
        try {
            $this->native->check($this->native->ffi->mdb_txn_commit($handle));
        } finally {
            $this->environment->release($this);
        }
    }

    public function abort(): void
    {
        if ($this->handle !== null) {
            $handle = $this->handle;
            $this->handle = null;
            try {
                $this->native->ffi->mdb_txn_abort($handle);
            } finally {
                $this->environment->release($this);
            }
        }
    }

    public function __destruct()
    {
        $this->abort();
    }
}
