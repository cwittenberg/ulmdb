# Derived once from source-bound reference observations; no runtime target dependencies.
for count in [1, 2, 3]:
    for readonly in [False, True]:
        for no in [False, True]:

            def rollback(db, spawn, row, readonly=readonly, no=no):
                p = spawn('rollback-owner')
                p.send('write-rollback undone first')
                p.send('read normal-rollback')
                p.close()
                p = spawn('previous-after-rollback', prev | (ro if readonly else 0) | (nolock if no else 0), allow_error=True)
                if p.ready:
                    p.send('read previous-after-rollback')
            case(f'rollback-seed{count}-readonly{int(readonly)}-nolock{int(no)}', count, rollback)
for count in [0, 1, 2, 3]:

    def normal_write(db, spawn, row):
        normal = spawn('normal-owner')
        normal.send('read before-previous')
        p = spawn('readonly-previous', prev | ro)
        p.send('read previous-start')
        normal.send('read normal-offset')
        normal.send('write normal-change second')
        normal.send('read normal-after-change')
        p.send('read previous-after-normal-change')
        p.send('info previous-flag-still-set')
    case(f'readonly-previous-normal-peer-dirty-seed{count}', count, normal_write)
