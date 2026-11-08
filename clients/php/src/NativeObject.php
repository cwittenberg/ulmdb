<?php
declare(strict_types=1);

namespace Ulmdb;

/** @internal Native capabilities cannot be copied or persisted. */
trait NativeObject
{
    private function __clone()
    {
    }

    public function __serialize(): array
    {
        throw new \LogicException('native database objects cannot be serialized');
    }

    public function __unserialize(array $data): void
    {
        throw new \LogicException('native database objects cannot be unserialized');
    }
}
