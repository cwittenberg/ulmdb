# Same24 source-observed schedules, adapted to the common owned case controller.
for count in [0, 1, 2, 3]:
    for hold in ['hold-writer', 'hold-clean', 'hold-prepare']:
        for finish in ['commit-held', 'abort-held']:
            def active(db, spawn, row, hold=hold, finish=finish):
                normal = spawn('normal-writer')
                normal.send(f'{hold} held-value second')
                previous = spawn('readonly-previous', prev | ro)
                previous.send('read while-writer-active')
                normal.send('read normal-reader-while-writer-active')
                normal.send(finish)
                normal.send('read normal-after-writer-end')
                previous.send('read previous-after-writer-end')
            case(f'seed{count}-{hold}-{finish}', count, active)
