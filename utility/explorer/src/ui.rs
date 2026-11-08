use crate::{
    app::{App, Focus, Input, Screen},
    files::EntryKind,
    text,
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Cell, Clear, List, ListItem, Padding, Paragraph, Row,
        Scrollbar, ScrollbarOrientation, ScrollbarState, Table, Wrap,
    },
};
use unicode_width::UnicodeWidthChar;

const BG: Color = Color::Rgb(17, 23, 31);
const FG: Color = Color::Rgb(230, 235, 241);
const MUTED: Color = Color::Rgb(156, 173, 191);
const ACCENT: Color = Color::Rgb(116, 214, 204);
const BORDER: Color = Color::Rgb(62, 79, 98);
const SELECTED: Color = Color::Rgb(34, 65, 77);
const ERROR: Color = Color::Rgb(255, 160, 148);

fn panel(title: impl Into<Line<'static>>, focused: bool) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if focused { ACCENT } else { BORDER }))
        .title(title)
        .title_style(
            Style::default()
                .fg(if focused { ACCENT } else { FG })
                .bold(),
        )
}

fn label(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::default().fg(ACCENT).bold())
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    frame.render_widget(Block::default().style(Style::default().bg(BG).fg(FG)), area);
    app.file_area = Rect::default();
    app.db_area = Rect::default();
    app.table_area = Rect::default();
    app.preview_area = Rect::default();
    if area.width < 50 || area.height < 15 {
        let action = if app.saving {
            "Finishing the transaction …"
        } else if app.editing.is_some() || app.deleting.is_some() {
            "Esc: cancel"
        } else {
            "q: quit"
        };
        frame.render_widget(Paragraph::new(format!("ulmdb explorer\n\nA little more room, please.\nResize to at least 50 columns × 15 rows.\n\n{action}")).style(Style::default().fg(FG)).wrap(Wrap { trim: false }), area);
        return;
    }
    let vertical = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(7),
        Constraint::Length(1),
    ])
    .split(area);
    let header = Line::from(vec![
        Span::styled(
            format!(" {}ulmdb explorer", app.icon("🔎 ", "")),
            Style::default().fg(FG).bold(),
        ),
        Span::raw("   "),
        Span::styled(
            if app.read_only {
                format!("{}read only", app.icon("🔒 ", ""))
            } else {
                String::new()
            },
            Style::default().fg(ACCENT),
        ),
        Span::styled(
            if app.loading { "   working …" } else { "" },
            Style::default().fg(MUTED),
        ),
    ]);
    let path = if app.screen == Screen::Files {
        app.browser.directory.as_path()
    } else {
        app.reader_path.as_deref().unwrap_or(&app.browser.directory)
    };
    frame.render_widget(
        Paragraph::new(vec![
            header,
            Line::styled(
                format!(" {}", text::escaped(path.to_string_lossy().as_bytes())),
                Style::default().fg(MUTED),
            ),
        ]),
        vertical[0],
    );
    match app.screen {
        Screen::Files => files(frame, app, vertical[1]),
        Screen::Records => records(frame, app, vertical[1]),
    }
    if app.error && !app.help && app.editing.is_none() && app.deleting.is_none() {
        let content = vertical[1];
        let notice = Rect::new(
            content.x,
            content.y + content.height.saturating_sub(6),
            content.width,
            content.height.min(6),
        );
        frame.render_widget(Clear, notice);
        frame.render_widget(
            Paragraph::new(text::escaped(app.status.as_bytes()))
                .wrap(Wrap { trim: true })
                .style(Style::default().bg(BG).fg(ERROR))
                .block(
                    panel(" Could not complete · r: refresh · o: open ", true)
                        .padding(Padding::horizontal(1)),
                ),
            notice,
        );
    }
    footer(frame, app, vertical[2]);
    if app.help {
        help(frame, app, area);
    }
    if app.editing.is_some() {
        edit_value(frame, app, area);
    } else if app.deleting.is_some() {
        delete_record(frame, app, area);
    }
}

fn files(frame: &mut Frame, app: &mut App, area: Rect) {
    let parts = if area.width >= 100 {
        Layout::horizontal([Constraint::Percentage(66), Constraint::Percentage(34)])
            .split(area)
            .to_vec()
    } else {
        vec![area]
    };
    let indices = app.file_indices();
    if app
        .file_state
        .selected()
        .is_some_and(|index| index >= indices.len())
    {
        app.file_state.select(indices.len().checked_sub(1));
    }
    let items: Vec<_> = indices
        .iter()
        .map(|&index| {
            let entry = &app.browser.entries[index];
            let (icon, suffix, color) = match entry.kind {
                EntryKind::Parent => (app.icon("↩ ", "< "), "  parent folder", MUTED),
                EntryKind::Directory => (app.icon("📁 ", "/ "), "/", FG),
                EntryKind::Database => (app.icon("🗃 ", "* "), "  [database]", ACCENT),
                EntryKind::File => (app.icon("📄 ", "  "), "", MUTED),
            };
            ListItem::new(Line::styled(
                format!(" {icon}{}{suffix}", text::escaped(entry.name.as_bytes())),
                Style::default().fg(color),
            ))
        })
        .collect();
    let title = format!(
        " {}Open a database · {} items{} ",
        app.icon("📂 ", ""),
        indices.len(),
        if app.browser.truncated {
            " · listing capped"
        } else {
            ""
        }
    );
    app.file_area = parts[0];
    let list = List::new(items)
        .block(panel(title, true))
        .highlight_style(Style::default().bg(SELECTED).fg(FG).bold())
        .highlight_symbol("› ");
    frame.render_stateful_widget(list, parts[0], &mut app.file_state);
    scrollbar(
        frame,
        parts[0],
        indices.len(),
        app.file_state.selected().unwrap_or(0),
    );
    if parts.len() > 1 {
        let copy = vec![
            Line::styled("A place for your data", Style::default().fg(FG).bold()),
            Line::raw(""),
            Line::raw(
                "Select a ULMDB folder or one of its files. Then choose a named database inside it.",
            ),
            Line::raw(""),
            Line::from(vec![label("Enter  "), Span::raw("Open selected item")]),
            Line::from(vec![label("↑ ↓    "), Span::raw("Move through files")]),
            Line::from(vec![label("⌫      "), Span::raw("Go to parent folder")]),
            Line::from(vec![label("/      "), Span::raw("Filter filenames")]),
            Line::from(vec![
                label("g      "),
                Span::raw("Type a folder or file path"),
            ]),
            Line::raw(""),
            Line::styled(
                "Browse, then make a change",
                Style::default().fg(ACCENT).bold(),
            ),
            Line::raw(
                "Select a record and press e to edit, or d to delete. Shift+Delete deletes immediately.",
            ),
            Line::raw(""),
            Line::styled(
                "Native ULMDB and existing adapter sidecars. Original LMDB files are never imported here.",
                Style::default().fg(MUTED),
            ),
        ];
        frame.render_widget(
            Paragraph::new(copy)
                .block(Block::default().padding(Padding::new(3, 1, 1, 0)))
                .wrap(Wrap { trim: true }),
            parts[1],
        );
    }
    if indices.is_empty() {
        frame.render_widget(
            Paragraph::new("No files match. Press Esc to clear the filter.")
                .style(Style::default().fg(MUTED))
                .wrap(Wrap { trim: true }),
            parts[0].inner(Margin {
                horizontal: 3,
                vertical: 2,
            }),
        );
    }
}

fn records(frame: &mut Frame, app: &mut App, area: Rect) {
    if area.width >= 76 {
        let detail_area = if app.show_databases {
            let parts =
                Layout::horizontal([Constraint::Length(25), Constraint::Min(30)]).split(area);
            databases(frame, app, parts[0]);
            parts[1]
        } else {
            area
        };
        if area.width >= 145 {
            let detail =
                Layout::horizontal([Constraint::Percentage(56), Constraint::Percentage(44)])
                    .split(detail_area);
            table(frame, app, detail[0]);
            preview(frame, app, detail[1]);
        } else {
            let detail = Layout::vertical([Constraint::Percentage(58), Constraint::Percentage(42)])
                .split(detail_area);
            table(frame, app, detail[0]);
            preview(frame, app, detail[1]);
        }
    } else if app.show_databases && app.focus == Focus::Databases {
        databases(frame, app, area);
    } else if app.focus == Focus::Preview {
        preview(frame, app, area);
    } else {
        table(frame, app, area);
    }
}

fn kind(kind: ulmdb::DatabaseKind) -> &'static str {
    match kind {
        ulmdb::DatabaseKind::Kv => "KV",
        ulmdb::DatabaseKind::Duplicates { .. } => "DUP",
        ulmdb::DatabaseKind::Ipv4 => "IPv4",
        ulmdb::DatabaseKind::Ipv6 => "IPv6",
    }
}

fn databases(frame: &mut Frame, app: &mut App, area: Rect) {
    app.db_area = area;
    let items: Vec<_> = app
        .databases
        .iter()
        .map(|database| {
            ListItem::new(Line::from(vec![
                Span::raw(format!(" {} ", text::escaped(&database.name))),
                Span::styled(
                    format!("({})", kind(database.kind)),
                    Style::default().fg(MUTED),
                ),
            ]))
        })
        .collect();
    let list = List::new(items)
        .block(panel(
            format!(
                " {}Databases · {} ",
                app.icon("🗃 ", ""),
                app.databases.len()
            ),
            app.focus == Focus::Databases,
        ))
        .highlight_style(Style::default().bg(SELECTED).fg(FG).bold())
        .highlight_symbol("› ");
    frame.render_stateful_widget(list, area, &mut app.db_state);
    scrollbar(
        frame,
        area,
        app.databases.len(),
        app.db_state.selected().unwrap_or(0),
    );
    if app.databases.is_empty() {
        frame.render_widget(
            Paragraph::new("No named databases yet.\n\no: open another file\nr: refresh")
                .style(Style::default().fg(MUTED))
                .wrap(Wrap { trim: true }),
            area.inner(Margin {
                horizontal: 2,
                vertical: 2,
            }),
        );
    }
}

fn table(frame: &mut Frame, app: &mut App, area: Rect) {
    app.table_area = area;
    let rows = app.records.iter().map(|record| {
        let key = text::escaped(&record.key);
        let bytes = &record.value[..record.value.len().min(160)];
        let value = if app.hex {
            text::hex(bytes)
        } else {
            text::escaped(bytes)
        };
        Row::new([
            Cell::from(key),
            Cell::from(value),
            Cell::from(record.value_len.to_string()).style(Style::default().fg(MUTED)),
        ])
    });
    let title = format!(
        " {}Records · page {}{} · {} ",
        app.icon("📋 ", ""),
        app.page_number + 1,
        if app.query.is_empty() {
            ""
        } else {
            " · filtered"
        },
        app.current_database()
            .map(|d| text::escaped(&d.name))
            .unwrap_or_default()
    );
    let header = Row::new(["KEY / PREFIX", "VALUE PREVIEW", "BYTES"])
        .style(Style::default().fg(MUTED).add_modifier(Modifier::BOLD));
    let widget = Table::new(
        rows,
        [
            Constraint::Percentage(40),
            Constraint::Min(8),
            Constraint::Length(7),
        ],
    )
    .header(header)
    .column_spacing(2)
    .block(panel(title, app.focus == Focus::Records))
    .row_highlight_style(Style::default().bg(SELECTED).fg(FG))
    .highlight_symbol("› ");
    frame.render_stateful_widget(widget, area, &mut app.table_state);
    scrollbar(
        frame,
        area,
        app.records.len(),
        app.table_state.selected().unwrap_or(0),
    );
    if app.records.is_empty() {
        let message = if app.loading {
            "Reading records …"
        } else if app.error {
            "Could not read this database. See the message below."
        } else if !app.complete {
            "No matches in this scan window.\n\nn: continue searching   /: change search"
        } else if !app.query.is_empty() {
            "No matching records.\n\n/: change search   Esc: clear search"
        } else {
            "This database is empty.\n\nr: refresh   o: open another file"
        };
        frame.render_widget(
            Paragraph::new(message)
                .style(Style::default().fg(MUTED))
                .wrap(Wrap { trim: true }),
            area.inner(Margin {
                horizontal: 3,
                vertical: 3,
            }),
        );
    }
}

fn preview(frame: &mut Frame, app: &mut App, area: Rect) {
    app.preview_area = area;
    let mut lines = Vec::new();
    if let Some(record) = app.current_record() {
        lines.push(Line::from(vec![
            label("Key  "),
            Span::raw(text::escaped(&record.key)),
        ]));
        lines.push(Line::styled(
            format!(
                "{} bytes{} · {}",
                record.value_len,
                if record.value_len > record.value.len() {
                    " · first 4096 shown"
                } else {
                    ""
                },
                if app.hex { "hex" } else { "text (escaped)" }
            ),
            Style::default().fg(MUTED),
        ));
        lines.push(Line::raw(""));
        if app.hex {
            for (index, chunk) in record.value.chunks(16).enumerate() {
                lines.push(Line::raw(format!(
                    "{:04x}  {}",
                    index * 16,
                    text::hex(chunk)
                )));
            }
        } else if record.value.is_empty() {
            lines.push(Line::styled("(empty value)", Style::default().fg(MUTED)));
        } else {
            for part in record.value.split(|byte| *byte == b'\n') {
                lines.push(Line::raw(text::escaped(part)));
            }
        }
    } else {
        lines.push(Line::styled(
            "Select a record to inspect its key and value.",
            Style::default().fg(MUTED),
        ));
    }
    let widget = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(panel(
            format!(" {}Value · x: text / hex ", app.icon("🔍 ", "")),
            app.focus == Focus::Preview,
        ));
    let height = widget.line_count(area.width.saturating_sub(2));
    app.preview_scroll = app.preview_scroll.min(
        height
            .saturating_sub(area.height as usize)
            .min(u16::MAX as usize) as u16,
    );
    frame.render_widget(widget.scroll((app.preview_scroll, 0)), area);
    scrollbar(
        frame,
        area,
        height.saturating_sub(2),
        app.preview_scroll as usize,
    );
}

fn scrollbar(frame: &mut Frame, area: Rect, length: usize, position: usize) {
    if length <= area.height.saturating_sub(2) as usize {
        return;
    }
    let widget = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_style(Style::default().fg(BORDER))
        .thumb_style(Style::default().fg(ACCENT));
    let mut state = ScrollbarState::new(length).position(position);
    frame.render_stateful_widget(
        widget,
        area.inner(Margin {
            horizontal: 0,
            vertical: 1,
        }),
        &mut state,
    );
}

fn popup(area: Rect, maximum_width: u16, maximum_height: u16) -> Rect {
    let width = area.width.saturating_sub(4).min(maximum_width);
    let height = area.height.saturating_sub(2).min(maximum_height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

fn edit_value(frame: &mut Frame, app: &mut App, area: Rect) {
    let popup = popup(area, 110, 34);
    frame.render_widget(Clear, popup);
    let Some(edit) = &mut app.editing else {
        return;
    };
    let mode = if edit.editor.hex { "hex" } else { "text" };
    let outer = panel(format!(" Edit value · {mode} "), true).style(Style::default().bg(BG).fg(FG));
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);
    let error_height = if edit.editor.error.is_some() { 3 } else { 0 };
    let regions = Layout::vertical([
        Constraint::Length(2 + error_height),
        Constraint::Min(2),
        Constraint::Length(1),
    ])
    .split(inner);
    let key = text::escaped(&edit.target.key);
    let database = text::escaped(&edit.target.database);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![label(" Database  "), Span::raw(database)]),
            Line::from(vec![label(" Key       "), Span::raw(key)]),
        ]),
        regions[0],
    );
    if let Some(error) = &edit.editor.error {
        let area = Rect::new(
            regions[0].x,
            regions[0].y + 2,
            regions[0].width,
            error_height,
        );
        frame.render_widget(
            Paragraph::new(text::escaped(error.as_bytes()))
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(ERROR)),
            area,
        );
    }
    let editor_area = regions[1];
    let (row, column) = edit.editor.cursor_position();
    let height = editor_area.height.max(1) as usize;
    let width = editor_area.width.max(1) as usize;
    let mut top = edit.editor.scroll as usize;
    let mut left = edit.editor.horizontal as usize;
    if row < top {
        top = row;
    } else if row >= top + height {
        top = row + 1 - height;
    }
    if column < left {
        left = column;
    } else if column >= left + width {
        left = column + 1 - width;
    }
    edit.editor.scroll = top.min(u16::MAX as usize) as u16;
    edit.editor.horizontal = left.min(u16::MAX as usize) as u16;
    frame.render_widget(
        Paragraph::new(edit.editor.text.as_str())
            .scroll((edit.editor.scroll, edit.editor.horizontal))
            .style(Style::default().bg(SELECTED).fg(FG)),
        editor_area,
    );
    if !app.saving && editor_area.width > 0 && editor_area.height > 0 {
        frame.set_cursor_position((
            editor_area.x + column.saturating_sub(left).min(width - 1) as u16,
            editor_area.y + row.saturating_sub(top).min(height - 1) as u16,
        ));
    }
    let controls = if app.saving {
        " Saving …"
    } else {
        " Ctrl+S save  Esc cancel  F2 text/hex"
    };
    frame.render_widget(Paragraph::new(Line::from(label(controls))), regions[2]);
}

fn delete_record(frame: &mut Frame, app: &App, area: Rect) {
    let popup = popup(area, 88, if app.error { 17 } else { 13 });
    frame.render_widget(Clear, popup);
    let Some(target) = &app.deleting else {
        return;
    };
    let note = if matches!(target.kind, ulmdb::DatabaseKind::Duplicates { .. }) {
        "Only this selected duplicate value will be deleted."
    } else {
        "This selected record will be deleted."
    };
    let outer = panel(" Confirm deletion · Esc cancels ", true)
        .style(Style::default().fg(FG).bg(BG))
        .padding(Padding::horizontal(1));
    let inner = outer.inner(popup);
    frame.render_widget(outer, popup);
    let regions = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(2),
        Constraint::Length(1),
    ])
    .split(inner);
    frame.render_widget(
        Paragraph::new("Delete this record?").style(Style::default().fg(ERROR).bold()),
        regions[0],
    );
    let message = vec![
        Line::from(vec![
            label("Database  "),
            Span::raw(text::escaped(&target.database)),
        ]),
        Line::from(vec![
            label("Key       "),
            Span::raw(text::escaped(&target.key)),
        ]),
        Line::from(vec![
            label("Value     "),
            Span::raw(text::escaped(
                &target.old_value[..target.old_value.len().min(100)],
            )),
        ]),
        Line::raw(""),
        Line::styled(note, Style::default().fg(MUTED)),
        Line::styled(
            if app.error {
                text::escaped(app.status.as_bytes())
            } else {
                String::new()
            },
            Style::default().fg(ERROR),
        ),
    ];
    frame.render_widget(
        Paragraph::new(message).wrap(Wrap { trim: true }),
        regions[1],
    );
    frame.render_widget(
        Paragraph::new(Line::styled(
            if app.saving {
                "Deleting …"
            } else {
                "Enter / y: delete   Esc / n: keep record"
            },
            Style::default().fg(ACCENT),
        )),
        regions[2],
    );
}

fn footer(frame: &mut Frame, app: &App, area: Rect) {
    if app.input != Input::None {
        let prompt = if app.input == Input::Path {
            " Path: "
        } else {
            " / "
        };
        let hint = " Esc cancel";
        let shown = text::escaped(app.input_text.as_bytes());
        let available =
            area.width
                .saturating_sub((prompt.len() + hint.len() + 2) as u16) as usize;
        let mut used = 0;
        let mut visible: Vec<char> = shown
            .chars()
            .rev()
            .take_while(|ch| {
                used += ch.width().unwrap_or(0);
                used <= available
            })
            .collect();
        visible.reverse();
        let shown: String = visible.into_iter().collect();
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                label(prompt),
                Span::raw(shown),
                label("▏"),
                Span::styled(hint, Style::default().fg(MUTED)),
            ])),
            area,
        );
        return;
    }
    if app.status.starts_with("Older page bookmarks") {
        frame.render_widget(
            Paragraph::new(Line::from(label(
                " Older page unavailable · Home: start · q quit",
            ))),
            area,
        );
        return;
    }
    let shortcuts = if app.screen == Screen::Files {
        if area.width < 76 {
            " Enter open  / find  g path  ? help  q quit"
        } else {
            " ↑↓ move  Enter open  / find  g path  r refresh  ? help  q quit"
        }
    } else if area.width < 76 {
        " e edit  d del  / find  b DBs  ? help  q quit"
    } else if area.width < 110 {
        " e edit  d del  Shift+Del now  / find  b DBs  ? help  q quit"
    } else {
        " e edit  d delete  Shift+Del now  / search  r refresh  b databases  Tab pane  ? help  q quit"
    };
    let note = if app.error {
        "Error".to_owned()
    } else if app.saving {
        "Saving…".into()
    } else if app.loading {
        "Working…".into()
    } else if app.status.starts_with("Value saved") {
        "Saved".into()
    } else if app.status.starts_with("Record deleted") {
        "Deleted".into()
    } else if app.screen == Screen::Records && app.records.is_empty() {
        if !app.complete {
            "n: continue search".into()
        } else if !app.query.is_empty() {
            "No matches".into()
        } else {
            "Empty".into()
        }
    } else {
        String::new()
    };
    let mut spans = vec![label(shortcuts)];
    if !note.is_empty()
        && shortcuts.chars().count() + note.chars().count() + 3 <= area.width as usize
    {
        spans.push(Span::styled(
            format!(" · {note}"),
            Style::default().fg(if app.error { ERROR } else { MUTED }),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn help(frame: &mut Frame, app: &mut App, area: Rect) {
    let width = area.width.saturating_sub(4).min(78);
    let height = area.height.saturating_sub(2).min(29);
    let popup = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    let lines = vec![
        Line::styled(
            "Everything you need to explore",
            Style::default().fg(FG).bold(),
        ),
        Line::raw(""),
        Line::raw("↑ ↓ / j k          Move selection or scroll value"),
        Line::raw("PageUp / PageDown  Move ten rows"),
        Line::raw("Enter              Open file / focus records / focus value"),
        Line::raw("Tab / Shift+Tab    Move between panes"),
        Line::raw("Mouse              Click to select; wheel to scroll"),
        Line::raw("/                  Search; Enter applies, Esc cancels"),
        Line::raw("Esc                Clear search / return to database list"),
        Line::raw("n / p              Next / previous record page"),
        Line::raw("Home / End         First page / last row on this page"),
        Line::raw("x                  Switch text and hex previews"),
        Line::raw("r / F5             Refresh snapshot or folder"),
        Line::raw("o                  Open another database folder or file"),
        Line::raw("g / Backspace      Type a path / parent folder (file picker)"),
        Line::raw("e                  Edit the selected record's value"),
        Line::raw("d / Delete         Delete the selected record (confirmation)"),
        Line::raw("Shift+Delete       Delete immediately, without confirmation"),
        Line::raw("b                  Hide / show the database sidebar"),
        Line::raw("Ctrl+S / Esc       Save / cancel in the value editor"),
        Line::raw("F2 / Ctrl+U        Text / hex mode; clear editor buffer"),
        Line::raw("q / Ctrl+C         Quit; edits commit only on explicit save"),
        Line::raw(""),
        Line::styled(
            "Search and large databases",
            Style::default().fg(ACCENT).bold(),
        ),
        Line::raw(
            "Case-insensitive text search checks keys and the first 4 KiB of values. Use hex:00ff to find binary bytes.",
        ),
        Line::raw(
            "Each page examines at most 4,096 records. Press n to continue a search window with no matches.",
        ),
        Line::raw(
            "Only 200 records are kept at once. Up to 128 page bookmarks are retained; Home starts over.",
        ),
        Line::raw(""),
        Line::styled(
            format!("{}Press Esc, Enter or ? to return", app.icon("✨ ", "")),
            Style::default().fg(ACCENT),
        ),
    ];
    let widget = Paragraph::new(lines)
        .wrap(Wrap { trim: true })
        .style(Style::default().fg(FG).bg(BG))
        .block(
            panel(" Keyboard guide · ↑↓ scroll · Esc close ", true).padding(Padding::horizontal(2)),
        );
    let content_height = widget.line_count(popup.width.saturating_sub(6));
    app.help_scroll = app.help_scroll.min(
        content_height
            .saturating_sub(popup.height as usize)
            .min(u16::MAX as usize) as u16,
    );
    frame.render_widget(widget.scroll((app.help_scroll, 0)), popup);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn rendered(app: &mut App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn file_picker_and_help_render_at_supported_sizes() {
        let mut app = App::new(Path::new(env!("CARGO_MANIFEST_DIR")), false, false).unwrap();
        for (width, height) in [(80, 24), (120, 36), (160, 45), (50, 15)] {
            let screen = rendered(&mut app, width, height);
            assert!(screen.contains("ulmdb explorer"));
            assert!(screen.contains("Open a database"));
            assert!(screen.contains("quit"));
            app.help = true;
            assert!(rendered(&mut app, width, height).contains("Keyboard guide"));
            app.help = false;
        }
        assert!(rendered(&mut app, 30, 8).contains("Resize"));
    }

    #[test]
    fn narrow_help_and_long_path_input_keep_the_last_content_reachable() {
        let mut app = App::new(Path::new(env!("CARGO_MANIFEST_DIR")), false, false).unwrap();
        app.help = true;
        app.help_scroll = 64;
        let screen = rendered(&mut app, 50, 15);
        assert!(screen.contains("to return"), "{screen}");
        app.help = false;
        app.input = Input::Path;
        app.input_text = format!("/{}TAIL-END", "folder/".repeat(50));
        let screen = rendered(&mut app, 50, 15);
        assert!(screen.contains("TAIL-END▏"), "{screen}");
    }

    #[test]
    fn record_panes_render_binary_data_safely_and_keep_quit_visible() {
        let mut app = App::new(Path::new(env!("CARGO_MANIFEST_DIR")), false, true).unwrap();
        app.screen = Screen::Records;
        app.databases = vec![crate::data::Database {
            name: b"hostnames".to_vec(),
            kind: ulmdb::DatabaseKind::Kv,
        }];
        app.db_state.select(Some(0));
        app.records = vec![crate::data::Record {
            key: b"binary-key".to_vec(),
            value: b"value\x1b[2J\xff".to_vec(),
            value_len: 11,
            duplicate_value: None,
        }];
        app.table_state.select(Some(0));
        for (width, height) in [(80, 24), (120, 36), (160, 45), (50, 15)] {
            for focus in [Focus::Databases, Focus::Records, Focus::Preview] {
                app.focus = focus;
                let screen = rendered(&mut app, width, height);
                assert!(screen.contains("ulmdb explorer"));
                assert!(screen.contains("q quit"), "{screen}");
                assert!(!screen.contains('\x1b'));
                if focus == Focus::Preview {
                    assert!(screen.contains("\\u{1b}"));
                }
            }
        }
    }

    #[test]
    fn edit_and_delete_controls_stay_visible_at_small_sizes() {
        let mut app = App::new(Path::new(env!("CARGO_MANIFEST_DIR")), false, false).unwrap();
        let target = crate::mutation::Target {
            database: b"records".to_vec(),
            kind: ulmdb::DatabaseKind::Kv,
            key: vec![b'k'; 512],
            old_value: vec![b'v'; 1000],
            commit: 0,
            uuid: [0; 16],
        };
        for (width, height) in [(50, 15), (80, 24), (120, 36)] {
            app.editing = Some(crate::app::EditSession {
                target: target.clone(),
                editor: crate::editor::Editor::new(b"draft\nsecond line", false),
            });
            let screen = rendered(&mut app, width, height);
            assert!(screen.contains("Ctrl+S"));
            assert!(screen.contains("cancel"));
            assert!(screen.contains("draft"));
            app.editing = None;
            app.deleting = Some(target.clone());
            let screen = rendered(&mut app, width, height);
            assert!(screen.contains("Enter / y: delete"), "{screen}");
            assert!(screen.contains("Esc / n: keep record"), "{screen}");
            app.deleting = None;
        }
    }

    #[test]
    fn one_line_footer_and_collapsible_sidebar_leave_room_for_records() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = App::new(Path::new(env!("CARGO_MANIFEST_DIR")), false, false).unwrap();
        app.screen = Screen::Records;
        app.focus = Focus::Records;
        app.databases = vec![crate::data::Database {
            name: b"hostnames".to_vec(),
            kind: ulmdb::DatabaseKind::Kv,
        }];
        app.db_state.select(Some(0));
        app.records = vec![crate::data::Record {
            key: b"key".to_vec(),
            value: b"value".to_vec(),
            value_len: 5,
            duplicate_value: None,
        }];
        app.table_state.select(Some(0));
        for (width, height) in [(80, 24), (120, 36)] {
            app.show_databases = true;
            let shown = rendered(&mut app, width, height);
            assert!(!shown.contains("edit enabled"));
            assert_eq!(app.db_area.height, height - 3);
            assert_eq!(app.preview_area.bottom(), height - 1);
            let original_width = app.table_area.width;
            app.key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
            assert!(!app.show_databases);
            let hidden = rendered(&mut app, width, height);
            assert!(hidden.contains("q quit"));
            assert_eq!(app.db_area.width, 0);
            assert_eq!(app.table_area.x, 0);
            assert!(app.table_area.width > original_width);
            assert_eq!(app.current_record().unwrap().key, b"key");
            app.input = Input::Search;
            app.input_text = "needle".into();
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
            let bottom: String = (0..width)
                .map(|x| terminal.backend().buffer()[(x, height - 1)].symbol())
                .collect();
            assert!(bottom.contains("needle"));
            assert!(bottom.contains("Esc cancel"));
            assert_eq!(app.preview_area.bottom(), height - 1);
            app.input = Input::None;
        }
    }

    use std::path::Path;
}
