<?php
declare(strict_types=1);

namespace Ulmdb;

/** A database failure; getCode() returns the LMDB-compatible integer code. */
final class Error extends \RuntimeException
{
}
