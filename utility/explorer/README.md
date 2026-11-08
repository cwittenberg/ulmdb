# ulmdb explorer

`uexplorer` is a visual terminal app for exploring and editing ULMDB. Pick a
database folder or file, select a named database, scroll through its records,
and inspect or edit values as text or hex. No query language or database server is needed.

## Run

From the repository root:

```sh
cargo run --release --locked -p uexplorer
cargo run --release --locked -p uexplorer -- /path/to/database
```

Without a path, the app opens a file picker in the current directory. A path may
be an environment directory, its `CURRENT` or segment file, or an existing
LMDB adapter sidecar. The window title inside the app is **ulmdb explorer**.

Build a standalone binary, or install it on your Cargo binary path:

```sh
cargo build --release --locked -p uexplorer
./target/release/uexplorer
cargo install --locked --path utility/explorer
uexplorer /path/to/database
```

Use `--no-emoji` for terminals without emoji fonts. `--help` and `--version` also
work without an interactive terminal. Normal use requires a real terminal.
Use `--read-only` to disable record editing and deletion.

To try the app with illustrative data, create a **new** demo directory:

```sh
cargo run --locked -p uexplorer --example demo -- utility/explorer/demo.ulmdb
./target/release/uexplorer utility/explorer/demo.ulmdb
```

The separate demo program creates 600 hostname records, binary and multiline
values, duplicate observations, and IPv4/IPv6 routes. The explorer does not
create environments; changes to existing records require explicit save or delete.

The generated `demo.ulmdb` directory is ignored by Git. If it already exists,
open it directly; the generator refuses to overwrite an existing environment.
To make another copy, pass a new directory to the example.

Try these in the explorer:

- In **hostnames**, scroll through 600 generated hosts or search for
  `host-0599` to jump to a match. **n** continues searching when necessary.
- Search for `welcome` to inspect a multiline message, or `binary-example`
  and press **x** to see the exact bytes in hex.
- Select **observations** to see three values for the same key.
- Select **routing4** and **routing6** to inspect IPv4/IPv6 CIDR records.
- Press **e** on a record to edit its value, **Ctrl+S** to save, or **Esc** to cancel.
- Press **d** to delete a selected record; confirm with **Enter** or cancel with **Esc**.

## Database filenames

Native ULMDB environments are directories, with no required filename extension.
The `.ulmdb` suffix on the demo folder is a naming convention. A typical native
file environment contains:

```text
demo.ulmdb/
  CURRENT
  commits/*.root
  segments/*.seg
```

`CURRENT` selects the committed snapshot; `.root` and `.seg` files are internal
parts of the environment. Open the directory or its `CURRENT` file in the
explorer. Keep the whole directory together when moving it.

## Navigate

The file picker marks likely database directories. Press **Enter** to open one,
then use the database list on the left, records in the center, and value preview.
On narrower terminals the records and preview stack; below 76 columns, **Tab**
switches between full-width panes. The minimum size is 50 columns by 15 rows.
Press **b** to hide/show the database sidebar and give its space to records.
The main bottom bar stays on one line; **?** opens the full keyboard guide.

| Control | Action |
| --- | --- |
| ↑ / ↓ or k / j | Move selection or scroll the focused value preview |
| PageUp / PageDown | Move ten rows |
| Enter | Open a file/folder, focus records, or focus the value |
| Tab / Shift+Tab | Switch panes |
| Mouse click / wheel | Select a row / scroll the pane under the pointer |
| / | Search records, or filter filenames in the picker |
| Enter / Esc while typing | Apply / cancel input |
| Ctrl+U while typing | Clear input |
| n / p | Next / previous record page |
| Home | Return to the first record page |
| End | Select the last row on the current page |
| x | Switch text and hex previews |
| e | Edit the selected record's complete value |
| d / Delete | Ask to delete the selected record |
| Shift+Delete | Delete immediately without a confirmation |
| b | Hide/show the database sidebar |
| Ctrl+S / Esc in editor | Save / cancel the draft |
| F2 in editor | Switch between text and hex editing |
| Ctrl+U in editor | Clear the value, including to save an empty value |
| r / F5 | Refresh the snapshot or file listing |
| o | Open another file or environment |
| g | Type a directory or file path in the picker |
| Backspace | Go to the parent folder in the picker |
| Esc | Clear the current search or return to the database pane |
| ? / F1 | Open keyboard help; ↑ / ↓ scrolls it |
| q / Ctrl+C / Ctrl+Q | Quit; Esc or Ctrl+C cancels an open editor/confirmation |

## Edit and delete

Select a record and press **e**. The editor loads its complete value, including
content beyond the 4 KiB browse preview. **Enter** inserts a new line, arrow keys
move the cursor, **Ctrl+S** saves, and **Esc** discards the draft. Keys and CIDR
prefixes remain unchanged. Ordinary text and Unicode can be edited directly;
binary values and hidden controls open in hex mode. **F2** switches formats when
the bytes can be represented exactly. Invalid hex remains in the editor with
an error, and is never saved. Terminal bracketed paste preserves multiline
Unicode text. Pasted hidden controls, including CR and tabs, are rejected with
the draft intact; represent such bytes explicitly in hex mode.

Press **d** or **Delete** to open a record deletion confirmation. **Enter** or
**y** deletes; **Esc** or **n** cancels. In a duplicates database, this removes
only the selected key/value pair. Replacing a duplicate with an already-existing
value is rejected rather than merging records.
**Shift+Delete** deletes the selected record immediately, without opening the
confirmation. It remains disabled with `--read-only` and while a change is in flight.

Both editing and deletion accept original values up to **64 KiB**. Larger
records remain browsable, but cannot be changed in this editor. Edited values
also have a 64 KiB limit; fixed-width duplicate databases retain their required
width. Large duplicate records retain their exact identity beyond the browse
preview, so two values with the same initial bytes cannot be confused.

Browsing still opens immutable snapshots. A writable transaction is acquired
only on save or an explicit deletion and released immediately after commit.
The transaction checks the original environment UUID and commit identity before
changing data. If any intervening commit occurred, the change is rejected and
the draft stays open: cancel, refresh, and reopen the record. An active native
writer can also make the operation return busy; its transaction must finish
first. Failed duplicate replacement rolls back the whole transaction.

After a successful change, the app refreshes the selected database and search
while retaining your page, row, scroll position, and focused pane. An edit
follows the edited record; deletion selects its successor in the same visual
slot, or the preceding record at the end. When a duplicate edit changes its sort
position, the app follows it in a bounded navigation window. If an edit no longer
matches the current search, the nearest remaining match stays selected.
Save errors preserve the draft; a committed change followed by a failed refresh
is reported separately. Quit/cancel input is held while a commit is in flight.
These are raw record changes; application-specific validation and secondary
index updates belong to the owning application.

## Search and resource bounds

Search is a case-insensitive substring match against record keys and the first
4 KiB of each value. For IP indexes, the key is the CIDR text. Use `hex:00ff` for
a binary byte search. Enter applies a search; typing does not repeatedly scan
the database. Search input is limited to 1,024 UTF-8 bytes.

Each page returns at most 200 records and examines at most 4,096 records. A scan
window can have no matches while more data remains: press **n** to continue.
The UI distinguishes an incomplete search from the end of a database. Matching
content beyond the first 4 KiB of a value is outside this search.

Only the current page's records and the latest 128 page bookmarks are retained.
Older pages can be reached again with **Home**. Values show their original byte
length, and previews are capped at 4 KiB. Binary bytes, control characters and
bidirectional control marks are escaped before display. Text mode renders line
breaks as separate lines; hex mode shows the exact preview bytes.

Database work runs on one reader thread. The UI remains available while a
snapshot opens or a page is scanned. Idle screens redraw only on input, resize,
or a completed read; pending reads show a working indicator.

## Supported storage and snapshots

Browsing uses `ReadTxn::open_snapshot`; saves/deletes use short cooperating
writer transactions. Legacy, Cohort and Indexed native file environments are supported, with
key/value, duplicate-value and IPv4/IPv6 databases. Persisted built-in key and
duplicate orderings are honored.

Existing adapter directories (`DATABASE/.ulmdb`) and single-file sidecars
(`FILE.ulmdb`) are discovered without running migration. An original LMDB file
without an existing native sidecar is unavailable here; the app explains how
to select a supported file instead of importing it.

Resident environments currently require exporting an immutable file snapshot
before inspection. Range formats, encrypted environments requiring a transform
context, and application-defined comparators are unavailable in this build.
The app shows the open/read error and keeps the previous view available.

Each successful open pins one immutable commit. **r** opens a new snapshot,
preserving the selected database and search when possible and returning to the
first page. An active writer may continue committing during browsing. Opening
a snapshot must be coordinated externally with pruning, reclamation, or
restoration: the independent reader API does not acquire a maintenance lease.
Snapshot admission validates the engine's selected graph, so initial open work
depends on the environment; the record page limits apply after opening.

## Development

```sh
cargo test --locked -p uexplorer
cargo clippy --locked -p uexplorer --all-targets --no-deps -- -D warnings
cargo fmt -p uexplorer -- --check
```

Tests cover actual on-disk databases, read-only snapshot behavior with an active
writer, refresh, all four database kinds, ordering and pagination, bounded
search, binary previews, file discovery, keyboard edit/save/delete journeys,
stale-write rejection, atomic duplicate updates, text/hex editing, and rendering at
multiple terminal sizes. Terminal dependencies belong to this utility crate;
the engine has no dependency on Ratatui or Crossterm.
