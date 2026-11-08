use crate::{
    data::{Database, Page, Position, Reader, Record},
    editor::Editor,
    files::{Browser, EntryKind},
    mutation::{self, Action, Target},
    text,
};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::{
    layout::Rect,
    widgets::{ListState, TableState},
};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

pub const PAGE_SIZE: usize = 200;
const BOOKMARK_LIMIT: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Files,
    Records,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Databases,
    Records,
    Preview,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    None,
    Search,
    Path,
}

enum Request {
    Open {
        id: u64,
        path: PathBuf,
    },
    Page {
        id: u64,
        database: Vec<u8>,
        after: Option<Position>,
        query: String,
    },
    Target {
        id: u64,
        database: Vec<u8>,
        record: Record,
        delete: bool,
        immediate: bool,
    },
    Mutate {
        id: u64,
        path: PathBuf,
        target: Target,
        action: Action,
    },
    Restore {
        id: u64,
        path: PathBuf,
        sequence: u64,
        context: RestoreContext,
    },
}
enum Response {
    Open {
        id: u64,
        result: Result<(PathBuf, u64, Vec<Database>), String>,
    },
    Page {
        id: u64,
        result: Result<Page, String>,
    },
    Target {
        id: u64,
        result: Result<Target, String>,
        delete: bool,
        immediate: bool,
    },
    Mutated {
        id: u64,
        result: Result<u64, String>,
        deleted: bool,
    },
    Restored {
        id: u64,
        result: Result<Box<Restored>, String>,
    },
}

struct RecordIdentity {
    key: Vec<u8>,
    value: Option<Vec<u8>>,
}

impl RecordIdentity {
    fn matches(&self, record: &Record) -> bool {
        self.key == record.key
            && self.value.as_deref().is_none_or(|value| {
                value == record.duplicate_value.as_deref().unwrap_or(&record.value)
            })
    }
}

struct RestoreContext {
    database: Vec<u8>,
    query: String,
    old_commit: u128,
    uuid: [u8; 16],
    desired: Option<RecordIdentity>,
    ordered_fallback: Option<RecordIdentity>,
    follow_exact: bool,
    selected: usize,
    table_state: TableState,
    preview_scroll: u16,
    bookmarks: VecDeque<Option<Position>>,
    first_bookmark: usize,
    page_number: usize,
}

struct Restored {
    path: PathBuf,
    sequence: u64,
    databases: Vec<Database>,
    page: Page,
    context: RestoreContext,
    selected: Option<usize>,
}

fn restore_reader(
    path: &Path,
    sequence: u64,
    mut context: RestoreContext,
) -> Result<(Reader, Box<Restored>), String> {
    let reader = Reader::open(path)?;
    reader.check_mutation_refresh(context.uuid, sequence)?;
    context
        .bookmarks
        .truncate(context.page_number - context.first_bookmark + 1);
    for position in context.bookmarks.iter_mut().flatten() {
        *position = reader.rebind_anchor(position, context.old_commit)?;
    }
    let load = |position: Option<&Position>| {
        reader.page(&context.database, position, &context.query, PAGE_SIZE)
    };
    let mut page = load(context.bookmarks.back().and_then(Option::as_ref))?;
    let mut selected = context
        .desired
        .as_ref()
        .and_then(|desired| page.records.iter().position(|row| desired.matches(row)));
    if selected.is_none()
        && context.follow_exact
        && let Some(desired) = &context.desired
    {
        let anchor = reader.anchor_around(
            &context.database,
            &desired.key,
            desired.value.as_deref(),
            &context.query,
            context.selected,
        )?;
        page = load(anchor.as_ref())?;
        selected = page.records.iter().position(|row| desired.matches(row));
        // This is a new bounded navigation window, not a claim about global rank.
        context.bookmarks = VecDeque::from([anchor]);
        context.first_bookmark = context.page_number;
    }
    let mut previous_page = false;
    while page.records.is_empty() && page.complete && context.bookmarks.len() > 1 {
        context.bookmarks.pop_back();
        context.page_number -= 1;
        page = load(context.bookmarks.back().and_then(Option::as_ref))?;
        previous_page = true;
    }
    if page.records.is_empty()
        && page.complete
        && let Some(removed) = &context.ordered_fallback
    {
        // A reordered duplicate may have established a one-row tail window
        // with no older bookmark. Seek its predecessor directly, still under
        // the same reverse scan bound; never leave a populated DB looking empty.
        let anchor = reader.anchor_around(
            &context.database,
            &removed.key,
            removed.value.as_deref(),
            &context.query,
            PAGE_SIZE,
        )?;
        page = load(anchor.as_ref())?;
        context.bookmarks = VecDeque::from([anchor]);
        context.first_bookmark = context.page_number;
        previous_page = true;
        // The empty forward window proved there are no matching rows beyond
        // this tail. The reverse window includes at most PAGE_SIZE matches.
        page.complete = true;
        page.resume = None;
    }
    if previous_page {
        selected = context
            .desired
            .as_ref()
            .and_then(|desired| page.records.iter().position(|row| desired.matches(row)));
    }
    if selected.is_none() {
        selected = if previous_page {
            page.records.len().checked_sub(1)
        } else {
            (!page.records.is_empty()).then(|| context.selected.min(page.records.len() - 1))
        };
    }
    let restored = Box::new(Restored {
        path: reader.path().to_path_buf(),
        sequence: reader.sequence(),
        databases: reader.databases().to_vec(),
        page,
        context,
        selected,
    });
    Ok((reader, restored))
}

pub struct EditSession {
    pub target: Target,
    pub editor: Editor,
}

pub struct App {
    pub browser: Browser,
    pub file_state: ListState,
    pub file_filter: String,
    pub reader_path: Option<PathBuf>,
    pub sequence: u64,
    pub databases: Vec<Database>,
    pub db_state: ListState,
    pub table_state: TableState,
    pub records: Vec<Record>,
    pub complete: bool,
    pub scanned: usize,
    pub page_number: usize,
    pub query: String,
    pub input: Input,
    pub input_text: String,
    pub screen: Screen,
    pub focus: Focus,
    pub preview_scroll: u16,
    pub hex: bool,
    pub emoji: bool,
    pub help: bool,
    pub help_scroll: u16,
    pub loading: bool,
    pub dirty: bool,
    pub quit: bool,
    pub read_only: bool,
    pub editing: Option<EditSession>,
    pub deleting: Option<Target>,
    pub saving: bool,
    pub show_databases: bool,
    pub status: String,
    pub error: bool,
    pub file_area: Rect,
    pub db_area: Rect,
    pub table_area: Rect,
    pub preview_area: Rect,
    resume: Option<Position>,
    bookmarks: VecDeque<Option<Position>>,
    first_bookmark: usize,
    request_id: u64,
    requests: Sender<Request>,
    responses: Receiver<Response>,
    reopen_database: Option<Vec<u8>>,
    select_last: bool,
    after_refresh: Option<String>,
    restore_context: Option<RestoreContext>,
}

impl App {
    pub fn new(path: &Path, explicit: bool, emoji: bool) -> std::io::Result<Self> {
        let browser_path = if path.is_dir() {
            path
        } else {
            path.parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
        };
        let browser = Browser::new(browser_path).or_else(|_| Browser::new(Path::new(".")))?;
        let (requests, receive) = mpsc::channel();
        let (send, responses) = mpsc::channel();
        thread::Builder::new()
            .name("uexplorer-reader".into())
            .spawn(move || {
                let mut reader: Option<Reader> = None;
                while let Ok(request) = receive.recv() {
                    let response = match request {
                        Request::Open { id, path } => {
                            let result = Reader::open(&path).map(|opened| {
                                let metadata = (
                                    opened.path().to_path_buf(),
                                    opened.sequence(),
                                    opened.databases().to_vec(),
                                );
                                reader = Some(opened);
                                metadata
                            });
                            Response::Open { id, result }
                        }
                        Request::Page {
                            id,
                            database,
                            after,
                            query,
                        } => {
                            let result = reader
                                .as_ref()
                                .ok_or_else(|| "Open a database first".into())
                                .and_then(|reader| {
                                    reader.page(&database, after.as_ref(), &query, PAGE_SIZE)
                                });
                            Response::Page { id, result }
                        }
                        Request::Target {
                            id,
                            database,
                            record,
                            delete,
                            immediate,
                        } => {
                            let result = reader
                                .as_ref()
                                .ok_or_else(|| "Open a database first".into())
                                .and_then(|reader| reader.target(&database, &record));
                            Response::Target {
                                id,
                                result,
                                delete,
                                immediate,
                            }
                        }
                        Request::Mutate {
                            id,
                            path,
                            target,
                            action,
                        } => {
                            let deleted = matches!(action, Action::Delete);
                            Response::Mutated {
                                id,
                                result: mutation::apply(&path, &target, action),
                                deleted,
                            }
                        }
                        Request::Restore {
                            id,
                            path,
                            sequence,
                            context,
                        } => {
                            let result = restore_reader(&path, sequence, context).map(
                                |(opened, restored)| {
                                    reader = Some(opened);
                                    restored
                                },
                            );
                            Response::Restored { id, result }
                        }
                    };
                    if send.send(response).is_err() {
                        break;
                    }
                }
            })?;
        let mut app = Self {
            browser,
            file_state: ListState::default().with_selected(Some(0)),
            file_filter: String::new(),
            reader_path: None,
            sequence: 0,
            databases: vec![],
            db_state: ListState::default(),
            table_state: TableState::default(),
            records: vec![],
            complete: true,
            scanned: 0,
            page_number: 0,
            query: String::new(),
            input: Input::None,
            input_text: String::new(),
            screen: Screen::Files,
            focus: Focus::Databases,
            preview_scroll: 0,
            hex: false,
            emoji,
            help: false,
            help_scroll: 0,
            loading: false,
            dirty: true,
            quit: false,
            read_only: false,
            editing: None,
            deleting: None,
            saving: false,
            show_databases: true,
            status: "Choose a ULMDB folder or file, then press Enter to open".into(),
            error: false,
            file_area: Rect::default(),
            db_area: Rect::default(),
            table_area: Rect::default(),
            preview_area: Rect::default(),
            resume: None,
            bookmarks: VecDeque::from([None]),
            first_bookmark: 0,
            request_id: 0,
            requests,
            responses,
            reopen_database: None,
            select_last: false,
            after_refresh: None,
            restore_context: None,
        };
        if explicit {
            app.open(path.to_path_buf());
        }
        Ok(app)
    }

    pub fn icon<'a>(&self, emoji: &'a str, plain: &'a str) -> &'a str {
        if self.emoji { emoji } else { plain }
    }

    pub fn file_indices(&self) -> Vec<usize> {
        let needle = self.file_filter.to_lowercase();
        self.browser
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.kind == EntryKind::Parent || e.name.to_lowercase().contains(&needle))
            .map(|(i, _)| i)
            .collect()
    }

    pub fn current_database(&self) -> Option<&Database> {
        self.db_state.selected().and_then(|i| self.databases.get(i))
    }
    pub fn current_record(&self) -> Option<&Record> {
        self.table_state
            .selected()
            .and_then(|i| self.records.get(i))
    }

    fn notify(&mut self, status: impl Into<String>, error: bool) {
        self.status = status.into();
        self.error = error;
        self.dirty = true;
    }

    pub fn open(&mut self, path: PathBuf) {
        if self.loading {
            return;
        }
        self.request_id += 1;
        self.loading = true;
        self.notify(
            format!(
                "Opening {} …",
                text::escaped(path.to_string_lossy().as_bytes())
            ),
            false,
        );
        if self
            .requests
            .send(Request::Open {
                id: self.request_id,
                path,
            })
            .is_err()
        {
            self.loading = false;
            self.notify("The reader stopped; quit and restart uexplorer", true);
        }
    }

    pub fn receive(&mut self) {
        while let Ok(response) = self.responses.try_recv() {
            match response {
                Response::Open { id, result } if id == self.request_id => {
                    self.loading = false;
                    match result {
                        Ok((path, sequence, databases)) => {
                            self.reader_path = Some(path);
                            self.sequence = sequence;
                            self.databases = databases;
                            let refreshing = self.reopen_database.is_some();
                            let previous = self.reopen_database.take();
                            if !refreshing {
                                self.query.clear();
                            }
                            let index = previous
                                .and_then(|name| self.databases.iter().position(|d| d.name == name))
                                .unwrap_or(0);
                            self.db_state
                                .select((!self.databases.is_empty()).then_some(index));
                            self.screen = Screen::Records;
                            self.focus = if self.after_refresh.is_some() || !self.show_databases {
                                Focus::Records
                            } else {
                                Focus::Databases
                            };
                            self.reset_pages();
                            self.request_page();
                            if self.databases.is_empty() {
                                self.notify("This environment has no named databases. Press o to open another file", false);
                            }
                        }
                        Err(error) => {
                            self.reopen_database = None;
                            let error = if let Some(saved) = self.after_refresh.take() {
                                format!("{saved}, but refresh failed: {error}")
                            } else {
                                error
                            };
                            self.notify(error, true);
                        }
                    }
                }
                Response::Page { id, result } if id == self.request_id => {
                    self.loading = false;
                    match result {
                        Ok(page) => {
                            self.records = page.records;
                            self.complete = page.complete;
                            self.resume = page.resume;
                            self.scanned = page.scanned;
                            let selected = if self.select_last {
                                self.records.len().checked_sub(1)
                            } else {
                                (!self.records.is_empty()).then_some(0)
                            };
                            self.select_last = false;
                            self.table_state = TableState::default().with_selected(selected);
                            self.preview_scroll = 0;
                            let status = if self.records.is_empty() && !self.complete {
                                "No matches in this scan window. Press n to continue searching"
                                    .into()
                            } else if self.records.is_empty() && !self.query.is_empty() {
                                "No matches. Press / to change the search or Esc to clear it".into()
                            } else if self.records.is_empty() {
                                "This database is empty. Press r to refresh".into()
                            } else {
                                format!(
                                    "{} records on this page · {} examined · {}",
                                    self.records.len(),
                                    self.scanned,
                                    if self.complete {
                                        "end of database"
                                    } else {
                                        "n for next page"
                                    }
                                )
                            };
                            let status = if let Some(saved) = self.after_refresh.take() {
                                format!("{saved} · {status}")
                            } else {
                                status
                            };
                            self.notify(status, false);
                        }
                        Err(error) => {
                            self.records.clear();
                            self.table_state.select(None);
                            let error = if let Some(saved) = self.after_refresh.take() {
                                format!("{saved}, but records could not refresh: {error}")
                            } else {
                                error
                            };
                            self.notify(error, true);
                        }
                    }
                }
                Response::Target {
                    id,
                    result,
                    delete,
                    immediate,
                } if id == self.request_id => {
                    self.loading = false;
                    match result {
                        Ok(target) => {
                            if immediate {
                                self.mutate(target, Action::Delete);
                            } else if delete {
                                self.deleting = Some(target);
                                self.notify(
                                    "Confirm deletion, or press Esc to keep this record",
                                    false,
                                );
                            } else {
                                self.editing = Some(EditSession {
                                    editor: Editor::new(&target.old_value, self.hex),
                                    target,
                                });
                                self.notify(
                                    "Edit the complete value · Ctrl+S saves · Esc cancels",
                                    false,
                                );
                            }
                        }
                        Err(error) => self.notify(error, true),
                    }
                }
                Response::Mutated {
                    id,
                    result,
                    deleted,
                } if id == self.request_id => {
                    self.saving = false;
                    self.loading = false;
                    match result {
                        Ok(sequence) => {
                            self.editing = None;
                            self.deleting = None;
                            self.after_refresh = Some(format!(
                                "{} at commit #{sequence}",
                                if deleted {
                                    "Record deleted"
                                } else {
                                    "Value saved"
                                }
                            ));
                            if let (Some(path), Some(context)) =
                                (self.reader_path.clone(), self.restore_context.take())
                            {
                                self.request_id += 1;
                                self.loading = true;
                                if self
                                    .requests
                                    .send(Request::Restore {
                                        id: self.request_id,
                                        path,
                                        sequence,
                                        context,
                                    })
                                    .is_err()
                                {
                                    self.loading = false;
                                    let saved = self.after_refresh.take().unwrap_or_default();
                                    self.notify(
                                        format!(
                                            "{saved}, but the reader stopped before refreshing"
                                        ),
                                        true,
                                    );
                                }
                            }
                        }
                        Err(error) => {
                            self.restore_context = None;
                            if let Some(edit) = &mut self.editing {
                                edit.editor.error = Some(error.clone());
                            }
                            self.notify(error, true);
                        }
                    }
                }
                Response::Restored { id, result } if id == self.request_id => {
                    self.loading = false;
                    match result {
                        Ok(restored) => {
                            let Restored {
                                path,
                                sequence,
                                databases,
                                page,
                                context,
                                selected,
                            } = *restored;
                            self.reader_path = Some(path);
                            self.sequence = sequence;
                            self.databases = databases;
                            self.db_state.select(
                                self.databases
                                    .iter()
                                    .position(|db| db.name == context.database),
                            );
                            self.records = page.records;
                            self.complete = page.complete;
                            self.resume = page.resume;
                            self.scanned = page.scanned;
                            self.table_state = context.table_state;
                            self.table_state.select(selected);
                            self.preview_scroll = context.preview_scroll;
                            if self.focus == Focus::Databases && !self.show_databases {
                                self.focus = Focus::Records;
                            }
                            self.page_number = context.page_number;
                            self.first_bookmark = context.first_bookmark;
                            self.bookmarks = context.bookmarks;
                            let saved = self.after_refresh.take().unwrap_or_default();
                            self.notify(
                                format!(
                                    "{saved} · position kept · {} records on this page",
                                    self.records.len()
                                ),
                                false,
                            );
                        }
                        Err(error) => {
                            let saved = self.after_refresh.take().unwrap_or_default();
                            self.notify(format!("{saved}, but refresh failed: {error}"), true);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn reset_pages(&mut self) {
        self.records.clear();
        self.resume = None;
        self.complete = true;
        self.scanned = 0;
        self.page_number = 0;
        self.first_bookmark = 0;
        self.bookmarks = VecDeque::from([None]);
        self.table_state = TableState::default();
        self.preview_scroll = 0;
        self.select_last = false;
    }

    fn request_page(&mut self) {
        let Some(database) = self.current_database().map(|d| d.name.clone()) else {
            return;
        };
        let after = self
            .bookmarks
            .get(self.page_number - self.first_bookmark)
            .cloned()
            .flatten();
        self.loading = true;
        self.request_id += 1;
        self.notify("Reading records …", false);
        if self
            .requests
            .send(Request::Page {
                id: self.request_id,
                database,
                after,
                query: self.query.clone(),
            })
            .is_err()
        {
            self.loading = false;
            self.notify("The reader stopped; quit and restart uexplorer", true);
        }
    }

    fn next_page(&mut self) {
        if self.loading || self.complete {
            return;
        }
        let Some(resume) = self.resume.clone() else {
            return;
        };
        self.page_number += 1;
        let index = self.page_number - self.first_bookmark;
        if index == self.bookmarks.len() {
            self.bookmarks.push_back(Some(resume));
        }
        if self.bookmarks.len() > BOOKMARK_LIMIT {
            self.bookmarks.pop_front();
            self.first_bookmark += 1;
        }
        self.request_page();
    }

    fn previous_page(&mut self) {
        if self.loading || self.page_number == 0 {
            return;
        }
        if self.page_number == self.first_bookmark {
            self.notify(
                "Older page bookmarks expired. Press Home to return to the start",
                false,
            );
            return;
        }
        self.page_number -= 1;
        self.request_page();
    }

    fn move_selection(&mut self, down: bool, amount: usize) {
        if self.screen == Screen::Files {
            let length = self.file_indices().len();
            if length > 0 {
                let next = moved(
                    self.file_state.selected().unwrap_or(0),
                    length,
                    down,
                    amount,
                );
                self.file_state.select(Some(next));
            }
        } else {
            match self.focus {
                Focus::Databases if !self.loading && !self.databases.is_empty() => {
                    let next = moved(
                        self.db_state.selected().unwrap_or(0),
                        self.databases.len(),
                        down,
                        amount,
                    );
                    if self.db_state.selected() != Some(next) {
                        self.db_state.select(Some(next));
                        self.query.clear();
                        self.reset_pages();
                        self.request_page();
                    }
                }
                Focus::Records if !self.loading => {
                    let selected = self.table_state.selected().unwrap_or(0);
                    if down && (self.records.is_empty() || selected == self.records.len() - 1) {
                        self.next_page();
                    } else if !down && selected == 0 && self.page_number > 0 {
                        self.select_last = self.page_number > self.first_bookmark;
                        self.previous_page();
                    } else if !self.records.is_empty() {
                        self.table_state.select(Some(moved(
                            selected,
                            self.records.len(),
                            down,
                            amount,
                        )));
                        self.preview_scroll = 0;
                    }
                }
                Focus::Preview => {
                    self.preview_scroll = if down {
                        self.preview_scroll.saturating_add(amount as u16).min(16384)
                    } else {
                        self.preview_scroll.saturating_sub(amount as u16)
                    };
                }
                _ => {}
            }
        }
        self.dirty = true;
    }

    fn activate_file(&mut self) {
        if self.loading {
            return;
        }
        let index = self.file_state.selected().unwrap_or(0);
        let Some(entry) = self
            .file_indices()
            .get(index)
            .and_then(|i| self.browser.entries.get(*i))
        else {
            return;
        };
        let path = entry.path.clone();
        if matches!(entry.kind, EntryKind::Parent | EntryKind::Directory) {
            self.change_directory(&path);
        } else {
            self.open(path);
        }
    }

    fn change_directory(&mut self, path: &Path) {
        match Browser::new(path) {
            Ok(browser) => {
                self.browser = browser;
                self.file_filter.clear();
                self.file_state = ListState::default().with_selected(Some(0));
                self.notify(
                    "Enter: open · Backspace: parent folder · g: type a path",
                    false,
                );
            }
            Err(error) => self.notify(format!("Cannot read folder: {error}"), true),
        }
    }

    fn prepare_mutation(&mut self, delete: bool, immediate: bool) {
        if self.read_only {
            self.notify(
                "Editing is disabled by --read-only. Restart without that option to edit records",
                true,
            );
            return;
        }
        if self.loading || self.screen != Screen::Records {
            return;
        }
        let Some(database) = self.current_database().map(|d| d.name.clone()) else {
            return;
        };
        let Some(record) = self.current_record().cloned() else {
            self.notify("Select a record first", false);
            return;
        };
        self.request_id += 1;
        self.loading = true;
        self.notify("Loading the complete selected value …", false);
        if self
            .requests
            .send(Request::Target {
                id: self.request_id,
                database,
                record,
                delete,
                immediate,
            })
            .is_err()
        {
            self.loading = false;
            self.notify("The reader stopped; quit and restart uexplorer", true);
        }
    }

    fn mutate(&mut self, target: Target, action: Action) {
        if self.read_only || self.loading {
            return;
        }
        let Some(path) = self.reader_path.clone() else {
            return;
        };
        let desired = match &action {
            Action::Replace(value) => Some(RecordIdentity {
                key: target.key.clone(),
                value: matches!(target.kind, ulmdb::DatabaseKind::Duplicates { .. })
                    .then(|| value.clone()),
            }),
            Action::Delete => None,
        };
        let follow_exact = matches!(target.kind, ulmdb::DatabaseKind::Duplicates { .. })
            && matches!(&action, Action::Replace(value) if Reader::matches_value(&self.query, &target.key, value).unwrap_or(false));
        self.restore_context = Some(RestoreContext {
            database: target.database.clone(),
            query: self.query.clone(),
            old_commit: target.commit,
            uuid: target.uuid,
            desired,
            ordered_fallback: matches!(
                target.kind,
                ulmdb::DatabaseKind::Kv | ulmdb::DatabaseKind::Duplicates { .. }
            )
            .then(|| RecordIdentity {
                key: target.key.clone(),
                value: matches!(target.kind, ulmdb::DatabaseKind::Duplicates { .. }).then(|| {
                    match &action {
                        Action::Replace(value) if follow_exact => value.clone(),
                        _ => target.old_value.clone(),
                    }
                }),
            }),
            follow_exact,
            selected: self.table_state.selected().unwrap_or(0),
            table_state: self.table_state,
            preview_scroll: self.preview_scroll,
            bookmarks: self.bookmarks.clone(),
            first_bookmark: self.first_bookmark,
            page_number: self.page_number,
        });
        self.request_id += 1;
        self.loading = true;
        self.saving = true;
        self.notify("Committing the change …", false);
        if self
            .requests
            .send(Request::Mutate {
                id: self.request_id,
                path,
                target,
                action,
            })
            .is_err()
        {
            self.loading = false;
            self.saving = false;
            self.restore_context = None;
            self.notify("The reader stopped; quit and restart uexplorer", true);
        }
    }

    pub fn key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        self.dirty = true;
        if self.saving {
            return;
        }
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        if self.editing.is_some() {
            if key.code == KeyCode::Esc || (control && matches!(key.code, KeyCode::Char('c' | 'q')))
            {
                self.editing = None;
                self.notify("Edit cancelled; no value was changed", false);
            } else if control && key.code == KeyCode::Char('s') {
                let edit = self.editing.as_ref().unwrap();
                match edit.editor.bytes() {
                    Ok(value) => {
                        let target = edit.target.clone();
                        self.mutate(target, Action::Replace(value));
                    }
                    Err(error) => {
                        self.editing.as_mut().unwrap().editor.error = Some(error);
                    }
                }
            } else if let Some(edit) = &mut self.editing {
                edit.editor.key(key);
            }
            return;
        }
        if self.deleting.is_some() {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('n' | 'q'))
                || (control && matches!(key.code, KeyCode::Char('c' | 'q')))
            {
                self.deleting = None;
                self.notify("Deletion cancelled; the record was kept", false);
            } else if matches!(key.code, KeyCode::Enter | KeyCode::Char('y')) {
                self.mutate(self.deleting.as_ref().unwrap().clone(), Action::Delete);
            }
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c' | 'q'))
        {
            self.quit = true;
            return;
        }
        if self.help {
            if matches!(
                key.code,
                KeyCode::Esc | KeyCode::Char('?' | 'q') | KeyCode::Enter
            ) {
                self.help = false;
            } else if matches!(
                key.code,
                KeyCode::Down | KeyCode::Char('j') | KeyCode::PageDown
            ) {
                self.help_scroll = self
                    .help_scroll
                    .saturating_add(if key.code == KeyCode::PageDown { 10 } else { 1 })
                    .min(64);
            } else if matches!(key.code, KeyCode::Up | KeyCode::Char('k') | KeyCode::PageUp) {
                self.help_scroll = self
                    .help_scroll
                    .saturating_sub(if key.code == KeyCode::PageUp { 10 } else { 1 });
            }
            return;
        }
        if self.input != Input::None {
            match key.code {
                KeyCode::Esc => {
                    self.input = Input::None;
                    self.input_text.clear();
                }
                KeyCode::Backspace => {
                    self.input_text.pop();
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.input_text.clear()
                }
                KeyCode::Char(ch)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                        && !ch.is_control() =>
                {
                    let limit = if self.input == Input::Search {
                        crate::data::MAX_QUERY_BYTES
                    } else {
                        4096
                    };
                    if self.input_text.len() + ch.len_utf8() <= limit {
                        self.input_text.push(ch);
                    }
                }
                KeyCode::Enter => {
                    let mode = self.input;
                    self.input = Input::None;
                    let value = std::mem::take(&mut self.input_text);
                    if mode == Input::Path {
                        let path = PathBuf::from(value);
                        if path.is_dir() {
                            self.change_directory(&path);
                            self.screen = Screen::Files;
                        } else {
                            self.open(path);
                        }
                    } else if self.screen == Screen::Files {
                        self.file_filter = value;
                        self.file_state = ListState::default().with_selected(Some(0));
                    } else {
                        self.query = value;
                        self.reset_pages();
                        self.focus = Focus::Records;
                        self.request_page();
                    }
                }
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Char('e') if self.screen == Screen::Records && !self.loading => {
                self.prepare_mutation(false, false)
            }
            KeyCode::Delete
                if key.modifiers.contains(KeyModifiers::SHIFT)
                    && self.screen == Screen::Records =>
            {
                if key.kind == KeyEventKind::Press && !self.loading {
                    self.prepare_mutation(true, true);
                }
            }
            KeyCode::Char('d') | KeyCode::Delete
                if self.screen == Screen::Records && !self.loading =>
            {
                self.prepare_mutation(true, false)
            }
            KeyCode::Char('b') if self.screen == Screen::Records => {
                self.show_databases = !self.show_databases;
                if self.show_databases {
                    self.focus = Focus::Databases;
                } else if self.focus == Focus::Databases {
                    self.focus = Focus::Records;
                }
            }
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') | KeyCode::F(1) => {
                self.help = true;
                self.help_scroll = 0;
            }
            KeyCode::Char('/') if !self.loading => {
                self.input = Input::Search;
                self.input_text = if self.screen == Screen::Files {
                    self.file_filter.clone()
                } else {
                    self.query.clone()
                };
            }
            KeyCode::Char('g') if self.screen == Screen::Files && !self.loading => {
                self.input = Input::Path;
                self.input_text = self.browser.directory.to_string_lossy().into_owned();
            }
            KeyCode::Char('o') if !self.loading => {
                self.screen = Screen::Files;
                self.file_area = Rect::default();
                self.notify(
                    "Choose another database folder or file · Esc returns to records",
                    false,
                );
            }
            KeyCode::Char('r') | KeyCode::F(5) if !self.loading => self.refresh(),
            KeyCode::Char('x') if self.screen == Screen::Records => {
                self.hex = !self.hex;
                self.preview_scroll = 0;
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(false, 1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(true, 1),
            KeyCode::PageDown => self.move_selection(true, 10),
            KeyCode::PageUp => self.move_selection(false, 10),
            KeyCode::Home if !self.loading => {
                if self.screen == Screen::Files {
                    self.file_state.select(Some(0));
                } else if self.focus == Focus::Records {
                    self.reset_pages();
                    self.request_page();
                } else if self.focus == Focus::Preview {
                    self.preview_scroll = 0;
                } else {
                    self.db_state
                        .select((!self.databases.is_empty()).then_some(0));
                    self.query.clear();
                    self.reset_pages();
                    self.request_page();
                }
            }
            KeyCode::End if self.screen == Screen::Files => {
                let length = self.file_indices().len();
                self.file_state.select(length.checked_sub(1));
            }
            KeyCode::End if self.screen == Screen::Records => {
                if self.focus == Focus::Records {
                    self.table_state.select(self.records.len().checked_sub(1));
                    self.preview_scroll = 0;
                }
            }
            KeyCode::Char('n') if self.screen == Screen::Records => self.next_page(),
            KeyCode::Char('p') if self.screen == Screen::Records => self.previous_page(),
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l')
                if self.screen == Screen::Records =>
            {
                self.focus = match self.focus {
                    Focus::Databases => Focus::Records,
                    Focus::Records => Focus::Preview,
                    Focus::Preview => {
                        if self.show_databases {
                            Focus::Databases
                        } else {
                            Focus::Records
                        }
                    }
                }
            }
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h')
                if self.screen == Screen::Records =>
            {
                self.focus = match self.focus {
                    Focus::Databases => Focus::Preview,
                    Focus::Records => {
                        if self.show_databases {
                            Focus::Databases
                        } else {
                            Focus::Preview
                        }
                    }
                    Focus::Preview => Focus::Records,
                }
            }
            KeyCode::Enter if self.screen == Screen::Files => self.activate_file(),
            KeyCode::Enter if self.screen == Screen::Records => {
                self.focus = match self.focus {
                    Focus::Databases => Focus::Records,
                    _ => Focus::Preview,
                }
            }
            KeyCode::Backspace if self.screen == Screen::Files && !self.loading => {
                if let Some(parent) = self.browser.directory.parent().map(Path::to_path_buf) {
                    self.change_directory(&parent);
                }
            }
            KeyCode::Esc if self.screen == Screen::Files => {
                if !self.file_filter.is_empty() {
                    self.file_filter.clear();
                    self.file_state.select(Some(0));
                } else if self.reader_path.is_some() {
                    self.screen = Screen::Records;
                }
            }
            KeyCode::Esc if self.screen == Screen::Records && !self.loading => {
                if !self.query.is_empty() {
                    self.query.clear();
                    self.reset_pages();
                    self.request_page();
                } else {
                    self.focus = if self.show_databases {
                        Focus::Databases
                    } else {
                        Focus::Records
                    };
                }
            }
            _ => {}
        }
    }

    fn refresh(&mut self) {
        if self.screen == Screen::Files {
            match self.browser.reload() {
                Ok(()) => {
                    self.file_state = ListState::default().with_selected(Some(0));
                    self.notify("Folder refreshed", false);
                }
                Err(error) => self.notify(format!("Cannot refresh folder: {error}"), true),
            }
        } else if let Some(path) = self.reader_path.clone() {
            self.reopen_database = self.current_database().map(|d| d.name.clone());
            self.open(path);
        }
    }

    pub fn paste(&mut self, value: &str) {
        if self.saving {
            return;
        }
        if let Some(edit) = &mut self.editing {
            edit.editor.paste(value);
        } else if self.input != Input::None {
            let limit = if self.input == Input::Search {
                crate::data::MAX_QUERY_BYTES
            } else {
                4096
            };
            for ch in value.chars().filter(|ch| {
                !ch.is_control() && !matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            }) {
                if self.input_text.len() + ch.len_utf8() > limit {
                    break;
                }
                self.input_text.push(ch);
            }
        }
        self.dirty = true;
    }

    pub fn mouse(&mut self, mouse: MouseEvent) {
        if self.help
            || self.input != Input::None
            || self.editing.is_some()
            || self.deleting.is_some()
        {
            return;
        }
        let point = (mouse.column, mouse.row).into();
        if self.screen == Screen::Records {
            if self.db_area.contains(point) {
                self.focus = Focus::Databases;
            } else if self.table_area.contains(point) {
                self.focus = Focus::Records;
            } else if self.preview_area.contains(point) {
                self.focus = Focus::Preview;
            }
        }
        match mouse.kind {
            MouseEventKind::ScrollDown => self.move_selection(true, 3),
            MouseEventKind::ScrollUp => self.move_selection(false, 3),
            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                if self.screen == Screen::Files && self.file_area.contains(point) {
                    let row = mouse.row.saturating_sub(self.file_area.y + 1) as usize;
                    let index = self.file_state.offset() + row;
                    if index < self.file_indices().len() {
                        self.file_state.select(Some(index));
                    }
                } else if self.focus == Focus::Databases
                    && self.db_area.contains(point)
                    && !self.loading
                {
                    let row = mouse.row.saturating_sub(self.db_area.y + 1) as usize;
                    let index = self.db_state.offset() + row;
                    if index < self.databases.len() && self.db_state.selected() != Some(index) {
                        self.db_state.select(Some(index));
                        self.query.clear();
                        self.reset_pages();
                        self.request_page();
                    }
                } else if self.focus == Focus::Records && self.table_area.contains(point) {
                    let row = mouse.row.saturating_sub(self.table_area.y + 2) as usize;
                    let index = self.table_state.offset() + row;
                    if index < self.records.len() {
                        self.table_state.select(Some(index));
                        self.preview_scroll = 0;
                    }
                }
                self.dirty = true;
            }
            _ => {}
        }
    }
}

fn moved(current: usize, length: usize, down: bool, amount: usize) -> usize {
    if down {
        current.saturating_add(amount).min(length.saturating_sub(1))
    } else {
        current.saturating_sub(amount)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::atomic::{AtomicU64, Ordering},
        time::{Duration, Instant},
    };
    use ulmdb::{DatabaseKind, Environment};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                "uexplorer-app-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            )))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn key(app: &mut App, code: KeyCode) {
        app.key(KeyEvent::new(code, KeyModifiers::NONE));
    }
    fn settle(app: &mut App) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.loading {
            app.receive();
            assert!(
                Instant::now() < deadline,
                "reader did not respond: {}",
                app.status
            );
            thread::sleep(Duration::from_millis(2));
        }
        assert!(!app.error, "{}", app.status);
    }
    fn search(app: &mut App, query: &str) {
        key(app, KeyCode::Char('/'));
        app.input_text = query.to_owned();
        key(app, KeyCode::Enter);
        settle(app);
    }

    #[test]
    fn keyboard_browsing_paging_search_refresh_and_quit_use_real_data() {
        let fixture = Fixture::new();
        let env = Environment::create(&fixture.0).unwrap();
        let mut writer = env.begin_write().unwrap();
        writer.create_database(b"first", DatabaseKind::Kv).unwrap();
        writer.create_database(b"second", DatabaseKind::Kv).unwrap();
        for number in 0..430 {
            writer
                .put(b"first", format!("k{number:04}").as_bytes(), b"value")
                .unwrap();
        }
        writer.put(b"second", b"other", b"second database").unwrap();
        writer.commit().unwrap();
        let mut app = App::new(&fixture.0, true, false).unwrap();
        settle(&mut app);
        assert_eq!(app.screen, Screen::Records);
        assert_eq!(app.records.len(), PAGE_SIZE);
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.focus, Focus::Records);
        key(&mut app, KeyCode::Char('n'));
        settle(&mut app);
        assert_eq!(app.page_number, 1);
        assert_eq!(app.records[0].key, b"k0200");
        key(&mut app, KeyCode::Up);
        settle(&mut app);
        assert_eq!(app.page_number, 0);
        assert_eq!(app.table_state.selected(), Some(PAGE_SIZE - 1));
        key(&mut app, KeyCode::Down);
        settle(&mut app);
        assert_eq!(app.records[0].key, b"k0200");
        key(&mut app, KeyCode::Char('p'));
        settle(&mut app);
        assert_eq!(app.records[0].key, b"k0000");
        search(&mut app, "K0429");
        assert_eq!(app.records.len(), 1);
        assert_eq!(app.records[0].key, b"k0429");
        key(&mut app, KeyCode::Char('x'));
        assert!(app.hex);
        let old_sequence = app.sequence;
        let mut writer = env.begin_write().unwrap();
        writer.put(b"first", b"k0429", b"updated value").unwrap();
        writer.commit().unwrap();
        assert_eq!(app.records[0].value, b"value");
        key(&mut app, KeyCode::Char('r'));
        settle(&mut app);
        assert!(app.sequence > old_sequence);
        assert_eq!(app.query, "K0429");
        assert_eq!(app.records[0].value, b"updated value");
        key(&mut app, KeyCode::Down);
        settle(&mut app);
        assert_eq!(app.current_database().unwrap().name, b"second");
        assert!(app.query.is_empty());
        assert_eq!(app.records[0].key, b"other");
        key(&mut app, KeyCode::Char('?'));
        assert!(app.help);
        key(&mut app, KeyCode::Down);
        assert_eq!(app.help_scroll, 1);
        key(&mut app, KeyCode::Esc);
        assert!(!app.help);
        key(&mut app, KeyCode::Char('q'));
        assert!(app.quit);
    }

    #[test]
    fn failed_opens_keep_existing_reader_and_search_input_accepts_quit_letters() {
        let fixture = Fixture::new();
        let env = Environment::create(&fixture.0).unwrap();
        let mut writer = env.begin_write().unwrap();
        writer
            .create_database(b"records", DatabaseKind::Kv)
            .unwrap();
        writer.put(b"records", b"key", b"value").unwrap();
        writer.commit().unwrap();
        let mut app = App::new(&fixture.0, true, false).unwrap();
        settle(&mut app);
        key(&mut app, KeyCode::Char('/'));
        key(&mut app, KeyCode::Char('q'));
        assert!(!app.quit);
        assert_eq!(app.input_text, "q");
        key(&mut app, KeyCode::Esc);
        app.open(fixture.0.join("missing"));
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.loading {
            app.receive();
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(2));
        }
        assert!(app.error);
        assert_eq!(app.records[0].key, b"key");
        assert_eq!(app.reader_path.as_ref(), Some(&fixture.0));
        key(&mut app, KeyCode::Char('r'));
        settle(&mut app);
        assert_eq!(app.records[0].key, b"key");
    }

    #[test]
    fn edit_save_cancel_and_confirmed_delete_persist_through_the_ui() {
        let fixture = Fixture::new();
        {
            let env = Environment::create(&fixture.0).unwrap();
            let mut write = env.begin_write().unwrap();
            write.create_database(b"records", DatabaseKind::Kv).unwrap();
            write.put(b"records", b"alpha", b"original").unwrap();
            write.put(b"records", b"neighbor", b"keep me").unwrap();
            write.commit().unwrap();
        }
        let mut app = App::new(&fixture.0, true, false).unwrap();
        settle(&mut app);
        let original_sequence = app.sequence;
        key(&mut app, KeyCode::Char('e'));
        settle(&mut app);
        assert_eq!(app.editing.as_ref().unwrap().editor.text, "original");
        app.editing.as_mut().unwrap().editor.text = "unsaved".into();
        key(&mut app, KeyCode::Esc);
        assert!(app.editing.is_none());
        assert_eq!(app.sequence, original_sequence);
        assert_eq!(
            ulmdb::ReadTxn::open_snapshot(&fixture.0)
                .unwrap()
                .get(b"records", b"alpha")
                .unwrap(),
            Some(b"original".as_slice())
        );
        key(&mut app, KeyCode::Char('e'));
        settle(&mut app);
        app.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        app.paste("edited 🌱\nsecond line");
        app.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        settle(&mut app);
        assert!(app.editing.is_none());
        assert!(app.sequence > original_sequence);
        assert!(app.status.contains("Value saved"));
        assert_eq!(app.records[0].value, "edited 🌱\nsecond line".as_bytes());
        key(&mut app, KeyCode::Char('d'));
        settle(&mut app);
        assert!(app.deleting.is_some());
        key(&mut app, KeyCode::Esc);
        assert!(app.deleting.is_none());
        assert_eq!(app.records.len(), 2);
        key(&mut app, KeyCode::Delete);
        settle(&mut app);
        key(&mut app, KeyCode::Enter);
        settle(&mut app);
        assert!(app.deleting.is_none());
        assert!(app.status.contains("Record deleted"));
        assert_eq!(app.records.len(), 1);
        assert_eq!(app.records[0].key, b"neighbor");
        let read = ulmdb::ReadTxn::open_snapshot(&fixture.0).unwrap();
        assert_eq!(read.get(b"records", b"alpha").unwrap(), None);
        assert_eq!(
            read.get(b"records", b"neighbor").unwrap(),
            Some(b"keep me".as_slice())
        );
    }

    fn numeric_fixture(count: usize) -> Fixture {
        let fixture = Fixture::new();
        {
            let env = Environment::create(&fixture.0).unwrap();
            let mut write = env.begin_write().unwrap();
            write.create_database(b"records", DatabaseKind::Kv).unwrap();
            for number in 0..count {
                write
                    .put(
                        b"records",
                        format!("k{number:04}").as_bytes(),
                        b"matching value",
                    )
                    .unwrap();
            }
            write.commit().unwrap();
        }
        fixture
    }

    fn save_draft(app: &mut App, value: &str) {
        key(app, KeyCode::Char('e'));
        settle(app);
        app.editing.as_mut().unwrap().editor.text = value.into();
        app.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        settle(app);
    }

    fn immediate_delete(app: &mut App) {
        app.key(KeyEvent::new(KeyCode::Delete, KeyModifiers::SHIFT));
        settle(app);
        assert!(app.deleting.is_none());
    }

    #[test]
    fn save_and_delete_preserve_later_page_viewport_focus_and_navigation() {
        let fixture = numeric_fixture(650);
        let mut app = App::new(&fixture.0, true, false).unwrap();
        settle(&mut app);
        app.focus = Focus::Records;
        key(&mut app, KeyCode::Char('n'));
        settle(&mut app);
        // Populate a forward bookmark, then edit on the earlier page.
        key(&mut app, KeyCode::Char('n'));
        settle(&mut app);
        key(&mut app, KeyCode::Char('p'));
        settle(&mut app);
        app.table_state.select(Some(53));
        *app.table_state.offset_mut() = 42;
        app.preview_scroll = 2;
        app.focus = Focus::Preview;
        save_draft(&mut app, "edited value");
        assert_eq!(app.page_number, 1);
        assert_eq!(app.current_record().unwrap().key, b"k0253");
        assert_eq!(app.table_state.selected(), Some(53));
        assert_eq!(app.table_state.offset(), 42);
        assert_eq!(app.focus, Focus::Preview);
        assert_eq!(app.preview_scroll, 2);
        assert_eq!(app.bookmarks.len(), 2, "future bookmarks must be discarded");
        immediate_delete(&mut app);
        assert_eq!(app.page_number, 1);
        assert_eq!(app.current_record().unwrap().key, b"k0254");
        assert_eq!(app.table_state.selected(), Some(53));
        assert_eq!(app.table_state.offset(), 42);
        assert_eq!(app.focus, Focus::Preview);
        assert_eq!(app.records.last().unwrap().key, b"k0400");
        key(&mut app, KeyCode::Char('n'));
        settle(&mut app);
        assert_eq!(
            app.records[0].key, b"k0401",
            "next page must use the new snapshot's resume"
        );
        key(&mut app, KeyCode::Char('p'));
        settle(&mut app);
        assert_eq!(app.records[0].key, b"k0200");
    }

    #[test]
    fn deletion_at_page_edges_keeps_successor_or_previous_page() {
        let fixture = numeric_fixture(401);
        let mut app = App::new(&fixture.0, true, false).unwrap();
        settle(&mut app);
        app.focus = Focus::Records;
        app.table_state.select(Some(199));
        *app.table_state.offset_mut() = 185;
        immediate_delete(&mut app);
        assert_eq!(app.current_record().unwrap().key, b"k0200");
        assert_eq!(app.table_state.selected(), Some(199));
        assert_eq!(app.table_state.offset(), 185);
        key(&mut app, KeyCode::Char('n'));
        settle(&mut app);
        assert_eq!(app.records[0].key, b"k0201");
        app.table_state.select(Some(199));
        immediate_delete(&mut app);
        assert_eq!(app.current_record().unwrap().key, b"k0399");
        assert_eq!(app.table_state.selected(), Some(198));

        let fixture = numeric_fixture(401);
        let mut app = App::new(&fixture.0, true, false).unwrap();
        settle(&mut app);
        key(&mut app, KeyCode::Char('n'));
        settle(&mut app);
        key(&mut app, KeyCode::Char('n'));
        settle(&mut app);
        assert_eq!(app.records.len(), 1);
        app.focus = Focus::Records;
        immediate_delete(&mut app);
        assert_eq!(app.page_number, 1);
        assert_eq!(app.current_record().unwrap().key, b"k0399");
        assert_eq!(app.table_state.selected(), Some(199));
        assert_eq!(app.focus, Focus::Records);
    }

    #[test]
    fn filtered_edit_that_loses_its_match_keeps_nearest_row_and_filter() {
        let fixture = numeric_fixture(430);
        let mut app = App::new(&fixture.0, true, false).unwrap();
        settle(&mut app);
        search(&mut app, "matching");
        key(&mut app, KeyCode::Char('n'));
        settle(&mut app);
        app.table_state.select(Some(17));
        *app.table_state.offset_mut() = 8;
        save_draft(&mut app, "no longer selected by the filter");
        assert_eq!(app.query, "matching");
        assert_eq!(app.page_number, 1);
        assert_eq!(app.table_state.selected(), Some(17));
        assert_eq!(app.table_state.offset(), 8);
        assert_eq!(app.current_record().unwrap().key, b"k0218");
    }

    #[test]
    fn duplicate_edit_follows_reordered_pair_without_unbounded_rescan() {
        let fixture = Fixture::new();
        {
            let env = Environment::create(&fixture.0).unwrap();
            let mut write = env.begin_write().unwrap();
            write
                .create_database(b"records", DatabaseKind::Duplicates { width: None })
                .unwrap();
            for number in 0..450 {
                write
                    .insert_duplicate(b"records", b"one key", format!("v{number:04}").as_bytes())
                    .unwrap();
            }
            write.commit().unwrap();
        }
        let mut app = App::new(&fixture.0, true, false).unwrap();
        settle(&mut app);
        key(&mut app, KeyCode::Char('n'));
        settle(&mut app);
        app.focus = Focus::Records;
        app.table_state.select(Some(35));
        *app.table_state.offset_mut() = 25;
        save_draft(&mut app, "a new first value");
        assert_eq!(app.current_record().unwrap().value, b"a new first value");
        assert_eq!(app.page_number, 1);
        assert_eq!(app.focus, Focus::Records);
        assert!(app.scanned <= crate::data::SCAN_LIMIT);
        assert_eq!(app.bookmarks.len(), 1);
        key(&mut app, KeyCode::Char('n'));
        settle(&mut app);
        assert_eq!(app.records[0].value, b"v0199");
    }

    #[test]
    fn deleting_only_row_after_duplicate_reanchor_selects_its_predecessor() {
        let fixture = Fixture::new();
        {
            let env = Environment::create(&fixture.0).unwrap();
            let mut write = env.begin_write().unwrap();
            write
                .create_database(b"records", DatabaseKind::Duplicates { width: None })
                .unwrap();
            for number in 0..450 {
                write
                    .insert_duplicate(b"records", b"one key", format!("v{number:04}").as_bytes())
                    .unwrap();
            }
            write.commit().unwrap();
        }
        let mut app = App::new(&fixture.0, true, false).unwrap();
        settle(&mut app);
        key(&mut app, KeyCode::Char('n'));
        settle(&mut app);
        assert_eq!(app.table_state.selected(), Some(0));
        app.focus = Focus::Preview;
        save_draft(&mut app, "z moved to the tail");
        assert_eq!(app.records.len(), 1);
        assert_eq!(app.bookmarks.len(), 1);
        immediate_delete(&mut app);
        assert_eq!(app.current_record().unwrap().value, b"v0449");
        assert_eq!(app.table_state.selected(), Some(PAGE_SIZE - 1));
        assert_eq!(app.focus, Focus::Preview);
        assert_eq!(app.page_number, 1);
        assert!(app.complete);
        assert!(app.scanned <= crate::data::SCAN_LIMIT);
    }

    #[test]
    fn shift_delete_is_immediate_press_only_and_respects_readonly() {
        let fixture = numeric_fixture(4);
        let mut app = App::new(&fixture.0, true, false).unwrap();
        settle(&mut app);
        app.read_only = true;
        app.key(KeyEvent::new(KeyCode::Delete, KeyModifiers::SHIFT));
        assert!(!app.loading);
        assert!(app.deleting.is_none());
        assert_eq!(app.records.len(), 4);
        app.read_only = false;
        let mut repeat = KeyEvent::new(KeyCode::Delete, KeyModifiers::SHIFT);
        repeat.kind = KeyEventKind::Repeat;
        app.key(repeat);
        assert!(!app.loading);
        assert_eq!(app.records.len(), 4);
        key(&mut app, KeyCode::Delete);
        settle(&mut app);
        assert!(app.deleting.is_some());
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.records.len(), 4);
        immediate_delete(&mut app);
        assert_eq!(app.records.len(), 3);
        assert_eq!(app.current_record().unwrap().key, b"k0001");
        app.key(repeat);
        assert!(!app.loading);
        assert_eq!(app.records.len(), 3);
    }

    #[test]
    fn prefix_pages_keep_exact_position_after_save_and_delete() {
        let fixture = Fixture::new();
        {
            let env = Environment::create(&fixture.0).unwrap();
            let mut write = env.begin_write().unwrap();
            write.create_database(b"v4", DatabaseKind::Ipv4).unwrap();
            write.create_database(b"v6", DatabaseKind::Ipv6).unwrap();
            for number in 0..260 {
                write
                    .insert_prefix(
                        b"v4",
                        ulmdb::Prefix::new(ulmdb::Address::V4(0x0a00_0000 + number * 256), 24)
                            .unwrap(),
                        b"route",
                    )
                    .unwrap();
                write
                    .insert_prefix(
                        b"v6",
                        ulmdb::Prefix::new(
                            ulmdb::Address::V6(
                                0x2001_0db8_0000_0000_0000_0000_0000_0000
                                    + ((number as u128) << 64),
                            ),
                            64,
                        )
                        .unwrap(),
                        b"route",
                    )
                    .unwrap();
            }
            write.commit().unwrap();
        }
        let mut app = App::new(&fixture.0, true, false).unwrap();
        settle(&mut app);
        for database in 0..2 {
            app.db_state.select(Some(database));
            app.reset_pages();
            app.request_page();
            settle(&mut app);
            key(&mut app, KeyCode::Char('n'));
            settle(&mut app);
            app.focus = Focus::Records;
            app.table_state.select(Some(40));
            *app.table_state.offset_mut() = 24;
            let selected_key = app.records[40].key.clone();
            let successor = app.records[41].key.clone();
            save_draft(&mut app, "changed route");
            assert_eq!(app.current_record().unwrap().key, selected_key);
            assert_eq!(app.page_number, 1);
            assert_eq!(app.table_state.offset(), 24);
            immediate_delete(&mut app);
            assert_eq!(app.current_record().unwrap().key, successor);
            assert_eq!(app.table_state.selected(), Some(40));
            assert_eq!(app.page_number, 1);
        }
    }

    #[test]
    fn reordered_duplicate_window_honors_persisted_reverse_value_order() {
        let fixture = Fixture::new();
        {
            let env = Environment::create(&fixture.0).unwrap();
            let mut write = env.begin_write().unwrap();
            write
                .create_database(b"records", DatabaseKind::Duplicates { width: None })
                .unwrap();
            write
                .set_database_ordering(
                    b"records",
                    ulmdb::ordering::DatabaseOrdering {
                        duplicates: ulmdb::ordering::OrderingSpec::ReverseBytes,
                        ..Default::default()
                    },
                )
                .unwrap();
            for number in 0..450 {
                write
                    .insert_duplicate(b"records", b"one key", format!("v{number:04}").as_bytes())
                    .unwrap();
            }
            write.commit().unwrap();
        }
        let mut app = App::new(&fixture.0, true, false).unwrap();
        settle(&mut app);
        key(&mut app, KeyCode::Char('n'));
        settle(&mut app);
        app.focus = Focus::Records;
        app.table_state.select(Some(35));
        *app.table_state.offset_mut() = 25;
        save_draft(&mut app, "last in reverse value order z");
        assert_eq!(
            app.current_record().unwrap().value,
            b"last in reverse value order z"
        );
        assert_eq!(app.table_state.selected(), Some(35));
        assert_eq!(app.table_state.offset(), 25);
        assert_eq!(app.focus, Focus::Records);
        assert!(app.scanned <= crate::data::SCAN_LIMIT);
    }

    #[test]
    fn readonly_blocks_mutations_and_stale_saves_keep_the_editor_buffer() {
        let fixture = Fixture::new();
        {
            let env = Environment::create(&fixture.0).unwrap();
            let mut write = env.begin_write().unwrap();
            write.create_database(b"records", DatabaseKind::Kv).unwrap();
            write.put(b"records", b"alpha", b"original").unwrap();
            write.commit().unwrap();
        }
        let mut app = App::new(&fixture.0, true, false).unwrap();
        settle(&mut app);
        app.read_only = true;
        key(&mut app, KeyCode::Char('e'));
        assert!(app.editing.is_none());
        assert!(!app.loading);
        assert!(app.status.contains("--read-only"));
        key(&mut app, KeyCode::Char('d'));
        assert!(app.deleting.is_none());
        app.read_only = false;
        key(&mut app, KeyCode::Char('e'));
        settle(&mut app);
        app.editing.as_mut().unwrap().editor.text = "my draft".into();
        {
            let env = Environment::open(&fixture.0).unwrap();
            let mut write = env.begin_write().unwrap();
            write.put(b"records", b"alpha", b"newer value").unwrap();
            write.commit().unwrap();
        }
        app.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.loading {
            app.receive();
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(2));
        }
        assert!(app.error);
        assert!(!app.saving);
        assert!(app.status.contains("changed"));
        assert_eq!(app.editing.as_ref().unwrap().editor.text, "my draft");
        let read = ulmdb::ReadTxn::open_snapshot(&fixture.0).unwrap();
        assert_eq!(
            read.get(b"records", b"alpha").unwrap(),
            Some(b"newer value".as_slice())
        );
    }
}
