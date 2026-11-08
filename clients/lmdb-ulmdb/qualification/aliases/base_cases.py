# Derived once from source-bound reference observations; no runtime target dependencies.
for count in [0, 1, 2, 3]:

    def peer_first(db, spawn, row):
        peer = spawn('normal-peer')
        peer.send('read before-previous')
        p = spawn('readonly-previous', prev | ro, allow_error=True)
        if p.ready:
            p.send('read previous-with-peer')
            p.send('info previous-with-peer')
        peer.send('read peer-still-normal')
    case(f'readonly-previous-existing-peer-seed{count}', count, peer_first)

    def writable_peer(db, spawn, row):
        peer = spawn('normal-peer')
        peer.send('read peer-before')
        p = spawn('writable-previous', prev, allow_error=True)
        if p.ready:
            p.send('read unexpected-success-recorded')
    case(f'writable-previous-existing-peer-seed{count}', count, writable_peer)
for count in [0, 1, 2, 3]:
    for no in [False, True]:

        def twice(db, spawn, row, no=no):
            flags = nolock if no else 0
            p = spawn('previous', prev | flags)
            p.send('read initial')
            p.send('clean')
            p.send('abort aborted aborted')
            p.send('read after-abort')
            p.send('prepare-abort discarded prepared')
            p.send('read after-prepared-abort')
            p.send('write C1 first')
            p.send('read after-first')
            p.send('write C2 second')
            p.send('read after-second')
            p.close()
            normal = spawn('normal-reopen', ro | flags)
            normal.send('read durable-after-second')
        case(f'second-dirty-seed{count}-nolock{int(no)}', count, twice)
for count in [0, 1, 2]:

    def fallback(db, spawn, row):
        p = spawn('previous', prev)
        p.send('write C1 first')
        p.send('read own-after-first')
        peer = spawn('readonly-peer', ro)
        peer.send('read readonly-peer')
        p.send('read own-after-peer')
        peer.close()
        p.send('read own-after-peer-close')
    case(f'readonly-peer-after-first-seed{count}', count, fallback)
for readonly in [False, True]:
    for kill in [False, True]:

        def retained(db, spawn, row, readonly=readonly, kill=kill):
            p = spawn('previous', prev | (ro if readonly else 0))
            p.send('read initial')
            peer = spawn('blocked-peer', ro, wait=False)
            peer.blocked('previous-open')
            if not readonly:
                for cmd in ['clean', 'abort aborted aborted', 'prepare-abort discarded prepared']:
                    p.send(cmd)
                    peer.blocked(cmd)
            if kill:
                p.kill()
            else:
                p.close()
            peer.opened()
            peer.send('read released-after-close')
        case(f'retained-ex-readonly{int(readonly)}-kill{int(kill)}', 2, retained)
for count in [0, 2, 3]:
    for kill in [False, True]:

        def reset(db, spawn, row, kill=kill):
            p = spawn('previous', prev)
            p.send('write C1 first')
            p.send('read own-offset')
            peer = spawn('surviving-normal-peer')
            peer.send('read peer-offset')
            if kill:
                p.kill()
            else:
                p.close()
            peer.send('read after-previous-exit')
            third = spawn('third-readonly', ro)
            third.send('read while-peer-survives')
            third.close()
            peer.close()
            last = spawn('normal-after-last-close', ro)
            last.send('read reset-from-durable')
        case(f'last-close-reset-seed{count}-kill{int(kill)}', count, reset)

        def alone(db, spawn, row, kill=kill):
            p = spawn('previous', prev)
            p.send('write C1 first')
            p.send('read own-offset')
            p.kill() if kill else p.close()
            last = spawn('reopen-alone', ro)
            last.send('read durable-alone')
        case(f'lone-owner-reset-seed{count}-kill{int(kill)}', count, alone)
for count in [1, 2, 3]:

    def rollback(db, spawn, row):
        normal = spawn('rollback-owner')
        normal.send('write-rollback undone first')
        normal.send('read rolled-back')
        normal.close()
        p = spawn('previous-after-rollback', prev, allow_error=True)
        if p.ready:
            p.send('read previous-after-rollback')
    case(f'previous-after-rollback-seed{count}', count, rollback)
for count in [0, 1, 3]:
    for compact in [False, True]:

        def backup(db, spawn, row, compact=compact):
            source = spawn('copy-owner')
            dest = Path(row['directory']) / 'backup'
            source.send(f'copy {dest} {int(compact)}')
            source.close()
            normal = spawn('normal-backup', ro, path=dest)
            normal.send('read backup-current')
            normal.close()
            p = spawn('previous-backup', prev | ro, path=dest, allow_error=True)
            if p.ready:
                p.send('read backup-previous')
        case(f'backup-boundary-seed{count}-compact{int(compact)}', count, backup)
