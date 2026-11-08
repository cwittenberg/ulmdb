<?php
declare(strict_types=1);

require dirname(__DIR__) . '/autoload.php';

// Keep all native owners alive in globals until PHP's shutdown destructors run.
// The environment was constructed first; its destructor must safely abort the
// retained transaction instead of throwing the explicit-close guard exception.
$environment = new Ulmdb\Environment($argv[1]);
$transaction = $environment->begin(write: true);
$database = $transaction->openDatabase('kv');
$database->put('key', 'uncommitted at process shutdown');
