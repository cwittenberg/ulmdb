<?php
declare(strict_types=1);

namespace Ulmdb;

use FFI;
use FFI\CData;

/** One embedded ULMDB environment. Finish transactions before closing it. */
final class Environment
{
    use NativeObject;

    private Native $native;
    private ?CData $handle = null;
    /** @var \WeakMap<Transaction, bool> */
    private \WeakMap $transactions;

    public function __construct(
        string $path,
        private readonly bool $readOnly = false,
        int $maxDatabases = 128,
        ?string $library = null
    ) {
        Native::text($path);
        if ($maxDatabases < 1 || $maxDatabases > 4096) {
            throw new \InvalidArgumentException('maxDatabases must be between 1 and 4096');
        }
        $this->transactions = new \WeakMap();
        $this->native = new Native($library);
        $handle = $this->native->ffi->new('MDB_env *');
        $this->native->check($this->native->ffi->mdb_env_create(FFI::addr($handle)));
        try {
            $this->native->check($this->native->ffi->mdb_env_set_maxdbs($handle, $maxDatabases));
            $this->native->check($this->native->ffi->mdb_env_open(
                $handle, $path, Native::NOTLS | ($readOnly ? Native::RDONLY : 0), 0600
            ));
            $this->handle = $handle;
        } catch (\Throwable $error) {
            $this->native->ffi->mdb_env_close($handle);
            throw $error;
        }
    }

    public function begin(bool $write = false): Transaction
    {
        if ($this->handle === null) {
            throw new \LogicException('environment is closed');
        }
        if ($write && $this->readOnly) {
            throw new \LogicException('environment is read-only');
        }
        if ($write) {
            foreach ($this->transactions as $active => $writing) {
                if ($writing) {
                    throw new \LogicException('this environment already has an active writer');
                }
            }
            // Do not retain the last WeakMap key beyond this scan.
            unset($active);
        }
        $handle = $this->native->ffi->new('MDB_txn *');
        $this->native->check($this->native->ffi->mdb_txn_begin(
            $this->handle, null, $write ? 0 : Native::RDONLY, FFI::addr($handle)
        ));
        $transaction = new Transaction($this, $this->native, $handle, $write);
        $this->transactions[$transaction] = $write;
        return $transaction;
    }

    public function read(callable $callback): mixed
    {
        $transaction = $this->begin();
        try {
            $result = $callback($transaction);
            if ($result instanceof \Generator) {
                throw new \LogicException('transaction callbacks must complete synchronously');
            }
            return $result;
        } finally {
            $transaction->abort();
        }
    }

    public function write(callable $callback): mixed
    {
        $transaction = $this->begin(write: true);
        try {
            $result = $callback($transaction);
            if ($result instanceof \Generator) {
                throw new \LogicException('transaction callbacks must complete synchronously');
            }
            $transaction->commit();
            return $result;
        } finally {
            $transaction->abort();
        }
    }

    /** @internal Called after the native transaction has been consumed. */
    public function release(Transaction $transaction): void
    {
        unset($this->transactions[$transaction]);
    }

    public function close(): void
    {
        if ($this->handle === null) {
            return;
        }
        if (count($this->transactions) !== 0) {
            throw new \LogicException('finish active transactions before closing the environment');
        }
        $this->native->ffi->mdb_env_close($this->handle);
        $this->handle = null;
    }

    public function __destruct()
    {
        // PHP shutdown may destroy an environment before its still-live
        // transactions, even though each transaction retains the environment.
        if ($this->handle === null) {
            return;
        }
        foreach ($this->transactions as $transaction => $writing) {
            $transaction->abort();
        }
        $this->close();
    }
}
