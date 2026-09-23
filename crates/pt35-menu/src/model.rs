//! The menu state machine: a stack of screens, each a filterable list.
//!
//! Deliberately free of Wayland and of any process spawning, so the whole
//! navigation model — including "what happens when you confirm a shutdown" —
//! is unit-testable.

use pt35_common::menu::{Adjust, Builtin, Kind, Layout, MenuTree, StateField};
use pt35_common::theme::Rgb;
use pt35_ui::keys::Key;
use pt35_ui::list::{ListState, Outcome};

/// A row or tile as the UI draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub label: String,
    pub note: String,
    /// A quick setting the D-pad changes in place.
    pub adjust: Option<Adjust>,
    /// A value read out on the right of the row.
    pub state: Option<StateField>,
    /// Set when the row opens a builtin screen, so the UI can replace `note`
    /// with live state.
    pub builtin: Option<Builtin>,
    pub glyph: String,
    /// freedesktop icon name, preferred over the glyph when the theme has it.
    pub icon: String,
    pub tint: Option<Rgb>,
    pub submenu: bool,
    /// The row that is current right now: connected, in use, picked.
    pub active: bool,
    /// Asks before it acts, because what it does is hard to undo.
    pub confirm: bool,
    /// What activating this row does, for a caller that needs to know. Empty
    /// on a row that came from `menu.toml` rather than a provider.
    pub payload: String,
}

/// One screen on the stack.
#[derive(Debug, Clone)]
pub struct Screen {
    pub title: String,
    pub list: ListState,
    pub source: Source,
    pub layout: Layout,
}

/// Where a screen's items came from, which decides what activating one does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A page from menu.toml, by id.
    Page(String),
    /// A dynamic list. The items carry their own payload, note and glyph; the
    /// payload is opaque to the model.
    Dynamic {
        builtin: Builtin,
        items: Vec<crate::providers::Item>,
    },
    /// A yes/no question guarding `command`.
    Confirm { command: Command },
}

/// What the UI layer is asked to actually do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Launch an app by its apps.toml id.
    App(String),
    /// Run a shell command.
    Exec(String),
    /// Hand these arguments to pt35ctl / pt35d.
    Action(String),
    /// Act on one item of a dynamic screen (focus a window, join a network…).
    Dynamic { builtin: Builtin, payload: String },
    /// A shipped helper script behind a quick-panel switch. Not an app: it is
    /// not remembered, not deduplicated, and does not hold the desktop's menu
    /// off waiting for a window that will never appear.
    Helper(String),
    /// An Appearance pick, `section.key=value`.
    Theme(String),
}

/// Result of feeding a key into the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Repaint.
    Redraw,
    /// Change a quick setting by one step, then stay on the screen.
    Adjust(Adjust, bool),
    /// Close the window this sway criteria matches, then stay on the screen.
    Close(String),
    /// Nothing changed.
    Nothing,
    /// The caller must populate this builtin screen and push it.
    Open(Builtin),
    /// Run this, then close the menu.
    Run(Command),
    /// Run this and stay: a toggle you want to watch change.
    RunStay(Command),
    /// Close the menu.
    Quit,
    /// Pin this launcher row to the top, or unpin it.
    TogglePin(String),
}

pub struct Model {
    tree: MenuTree,
    stack: Vec<Screen>,
    rows: usize,
    grid_rows: usize,
    columns: usize,
    /// Depth of the screen the menu was opened on, when that was not the root:
    /// the picker from R, Power from the power key. Back there means "put me
    /// back where I was", not "show me what is underneath".
    entry: usize,
}

impl Model {
    /// Open `page`, or the tree's root when `None`.
    /// `rows` is list rows. `grid_rows` x `columns` is one page of tiles.
    pub fn sized(
        tree: MenuTree,
        page: Option<&str>,
        rows: usize,
        grid_rows: usize,
        columns: usize,
    ) -> Self {
        let mut model = Self {
            tree,
            stack: Vec::new(),
            rows: rows.max(1),
            grid_rows: grid_rows.max(1),
            columns: columns.max(1),
            entry: 1,
        };
        let root = page
            .map(str::to_string)
            .unwrap_or_else(|| model.tree.root.clone());
        model.push_page(&root);
        model
    }

    pub fn screen(&self) -> &Screen {
        self.stack
            .last()
            .expect("the stack always holds at least one screen")
    }

    pub fn screen_mut(&mut self) -> &mut Screen {
        self.stack
            .last_mut()
            .expect("the stack always holds at least one screen")
    }

    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// The rows to draw: their label and whether they lead somewhere deeper
    /// (which earns a chevron).
    pub fn visible_rows(&self) -> Vec<Row> {
        let screen = self.screen();
        screen
            .list
            .window()
            .into_iter()
            .map(|(index, label)| {
                let entry = self.entry(&screen.source, index);
                let item = match &screen.source {
                    Source::Dynamic { items, .. } => items.get(index),
                    _ => None,
                };
                Row {
                    label: label.to_string(),
                    note: entry
                        .map(|e| e.note.clone())
                        .or_else(|| item.map(|i| i.note.clone()))
                        .unwrap_or_default(),
                    builtin: entry.and_then(|e| e.builtin),
                    adjust: entry.and_then(|e| e.adjust),
                    state: entry.and_then(|e| e.state),
                    glyph: entry
                        .map(|e| e.glyph.clone())
                        .or_else(|| item.map(|i| i.glyph.clone()))
                        .filter(|g| !g.is_empty())
                        .unwrap_or_else(|| {
                            label
                                .chars()
                                .next()
                                .unwrap_or('?')
                                .to_uppercase()
                                .to_string()
                        }),
                    icon: entry
                        .map(|e| e.icon.clone())
                        .or_else(|| item.map(|i| i.icon.clone()))
                        .unwrap_or_default(),
                    tint: entry
                        .and_then(|e| e.tint)
                        .or_else(|| item.and_then(|i| i.tint)),
                    submenu: self.leads_deeper(&screen.source, index),
                    active: item.is_some_and(|i| i.active),
                    confirm: entry.is_some_and(|e| e.confirm),
                    // A menu.toml row carries its action, so the screen can
                    // tell which of several is the one in effect.
                    payload: item
                        .map(|i| i.payload.clone())
                        .or_else(|| {
                            entry
                                .and_then(|e| e.action.clone())
                                .map(|a| format!("action:{a}"))
                        })
                        .unwrap_or_default(),
                }
            })
            .collect()
    }

    /// The quick setting under the cursor, if this row is one.
    pub fn focused_adjust(&self) -> Option<Adjust> {
        self.adjust_at(self.screen().list.selected()?)
    }

    /// The quick setting on one row, whether or not the cursor is on it.
    ///
    /// A tap needs this: the slider under your finger is not the selected one.
    pub fn adjust_at(&self, index: usize) -> Option<Adjust> {
        let screen = self.screen();
        if let Source::Dynamic { items, .. } = &screen.source {
            let payload = items.get(index).map(|i| i.payload.as_str())?;
            return match payload.strip_prefix("adjust:")? {
                "volume" => Some(Adjust::Volume),
                "brightness" => Some(Adjust::Brightness),
                "scale" => Some(Adjust::Scale),
                _ => None,
            };
        }
        self.entry(&screen.source, index)?.adjust
    }

    /// Tiles or rows for the screen on top.
    pub fn layout(&self) -> Layout {
        self.screen().layout
    }

    fn entry(&self, source: &Source, index: usize) -> Option<&pt35_common::menu::Entry> {
        let Source::Page(id) = source else {
            return None;
        };
        self.tree.page(id)?.entries.get(index)
    }

    fn leads_deeper(&self, source: &Source, index: usize) -> bool {
        let Source::Page(id) = source else {
            return false;
        };
        let Some(page) = self.tree.page(id) else {
            return false;
        };
        matches!(
            page.entries.get(index).map(|entry| entry.kind()),
            Some(Ok(Kind::Goto(_))) | Some(Ok(Kind::Builtin(_)))
        )
    }

    fn push_page(&mut self, id: &str) {
        let (title, labels, layout) = match self.tree.page(id) {
            Some(page) => (
                if page.title.is_empty() {
                    id.to_string()
                } else {
                    page.title.clone()
                },
                page.entries
                    .iter()
                    .map(|e| e.label.clone())
                    .collect::<Vec<_>>(),
                page.layout,
            ),
            None => (
                "missing menu".to_string(),
                vec![format!("no menu {id:?} in menu.toml")],
                Layout::List,
            ),
        };
        let list = match layout {
            Layout::Grid => ListState::new(labels, self.grid_rows).with_columns(self.columns),
            Layout::List => ListState::new(labels, self.rows),
        };
        self.stack.push(Screen {
            title,
            list,
            source: Source::Page(id.to_string()),
            layout,
        });
    }

    /// Open a page from menu.toml by id, on top of what is there.
    pub fn open_page(&mut self, id: &str) {
        self.push_page(id);
    }

    /// Make a builtin the bottom of the stack. The launcher is the root, and
    /// Back from the first screen must leave the menu, not reveal a stub.
    pub fn set_root_dynamic(
        &mut self,
        builtin: Builtin,
        title: &str,
        items: Vec<crate::providers::Item>,
    ) {
        self.stack.clear();
        self.push_dynamic(builtin, title, items);
    }

    /// Push a screen built from live data (windows, networks, .desktop files).
    pub fn push_dynamic(
        &mut self,
        builtin: Builtin,
        title: &str,
        items: Vec<crate::providers::Item>,
    ) {
        let layout = crate::providers::screen(builtin).layout;
        let mut list = self.list_for(builtin, layout, &items);
        // The picker opens on the window you are in, centred, so you see
        // where you are before you move. Never on the Launcher card in front.
        if builtin == Builtin::Windows && items.len() > 1 {
            list.select_item(picker_start(&items));
        }
        self.stack.push(Screen {
            title: title.to_string(),
            list,
            layout,
            source: Source::Dynamic { builtin, items },
        });
    }

    fn list_for(
        &self,
        builtin: Builtin,
        layout: Layout,
        items: &[crate::providers::Item],
    ) -> ListState {
        let labels: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
        // A search also finds an app by its id or command: "yazi" finds Files.
        // Without the `app:` / `exec:` in front: "ap" must not match every
        // profiled app before the one whose name holds it.
        let keywords: Vec<String> = items
            .iter()
            .map(|i| {
                let p = i.payload.as_str();
                p.split_once(':').map_or(p, |(_, rest)| rest).to_string()
            })
            .collect();
        match layout {
            Layout::Grid => ListState::new(labels, self.grid_rows).with_columns(self.columns),
            // The quick panel is one screenful by design: it never scrolls.
            // The switcher strip draws every card and scrolls itself.
            Layout::List if matches!(builtin, Builtin::Quick | Builtin::Windows) => {
                let rows = labels.len().max(1);
                ListState::new(labels, rows).with_keywords(keywords)
            }
            Layout::List => ListState::new(labels, self.rows).with_keywords(keywords),
        }
    }

    /// Put a confirmation in front of closing every window.
    fn confirm_close_all(&mut self) -> Step {
        let count = match &self.screen().source {
            Source::Dynamic { items, .. } => items
                .iter()
                .filter(|item| item.payload != crate::providers::HOME)
                .count(),
            _ => 0,
        };
        if count == 0 {
            return Step::Nothing;
        }
        let label = match count {
            1 => "Close 1 window".to_string(),
            n => format!("Close all {n} windows"),
        };
        self.push_confirm(Command::Action("window closeall".into()), &label);
        Step::Redraw
    }

    fn push_confirm(&mut self, command: Command, label: &str) {
        let rows = self.rows;
        // Spelled out, not "No" and "Yes": half a second of reading is the
        // point of a confirmation, and the row says what it does without a
        // glance back at the title.
        let no = "No, go back".to_string();
        let yes = format!("Yes, {}", label.to_lowercase());
        self.stack.push(Screen {
            title: format!("{label}?"),
            list: ListState::new(vec![no, yes], rows),
            source: Source::Confirm { command },
            layout: Layout::List,
        });
    }

    /// Drop back to the top screen, keeping the menu open.
    pub fn go_home(&mut self) -> Step {
        if self.stack.len() > 1 {
            self.stack.truncate(1);
            self.forget_entry();
            Step::Redraw
        } else {
            Step::Nothing
        }
    }

    /// Every screen's title, the top one first: the breadcrumb trail.
    pub fn trail(&self) -> Vec<&str> {
        self.stack.iter().map(|s| s.title.as_str()).collect()
    }

    /// Back to the screen at this depth, 1 being the top. A breadcrumb click.
    pub fn back_to(&mut self, depth: usize) -> Step {
        if depth == 0 || depth >= self.stack.len() {
            return Step::Nothing;
        }
        self.stack.truncate(depth);
        self.forget_entry();
        Step::Redraw
    }

    /// Put the cursor on the row with this payload, wherever it now sits.
    pub fn focus_payload(&mut self, payload: &str) {
        let Some(index) = (match &self.screen().source {
            Source::Dynamic { items, .. } => items.iter().position(|i| i.payload == payload),
            _ => None,
        }) else {
            return;
        };
        self.screen_mut().list.select_item(index);
    }

    /// Remember the screen on top as the one the menu was opened on.
    pub fn mark_entry(&mut self) {
        self.entry = self.stack.len();
    }

    /// Once you have gone above it, the screen you came in on is just a screen.
    fn forget_entry(&mut self) {
        if self.stack.len() < self.entry {
            self.entry = 1;
        }
    }

    /// Whether Back leaves the menu from here, rather than going up a screen.
    pub fn back_closes(&self) -> bool {
        self.entry > 1 && self.stack.len() == self.entry
    }

    /// The menu.toml page in front, or the one a yes/no question on top came
    /// from. `None` on a dynamic screen.
    pub fn page_name(&self) -> Option<&str> {
        self.stack
            .iter()
            .rev()
            .find(|s| !matches!(s.source, Source::Confirm { .. }))
            .and_then(|s| match &s.source {
                Source::Page(name) => Some(name.as_str()),
                _ => None,
            })
    }

    /// True on a screen opened straight over an app, or one reached from it:
    /// the launcher is underneath, but not what you are looking at.
    pub fn over_app(&self) -> bool {
        self.entry > 1 && self.stack.len() >= self.entry
    }

    /// True when the bottom of the stack is the launcher.
    pub fn launcher_is_root(&self) -> bool {
        matches!(
            self.stack.first().map(|s| &s.source),
            Some(Source::Dynamic {
                builtin: Builtin::Launcher,
                ..
            })
        )
    }

    /// Payload of the row under the cursor on a dynamic screen.
    pub fn selected_payload(&self) -> Option<&str> {
        let Source::Dynamic { items, .. } = &self.screen().source else {
            return None;
        };
        let index = self.screen().list.selected()?;
        items.get(index).map(|item| item.payload.as_str())
    }

    /// Whether this screen is asking you to confirm something.
    ///
    /// Such a screen drops its 1-9 shortcuts, because `2` would be an instant
    /// yes to something you were being asked to think about, and its search,
    /// because there is nothing to search in two rows.
    pub fn is_confirm(&self) -> bool {
        matches!(
            self.stack.last().map(|s| &s.source),
            Some(Source::Confirm { .. })
        )
    }

    /// The builtin behind the current screen, if it is a dynamic one.
    pub fn dynamic_builtin(&self) -> Option<Builtin> {
        match self.stack.last().map(|screen| &screen.source) {
            Some(Source::Dynamic { builtin, .. }) => Some(*builtin),
            _ => None,
        }
    }

    /// Refresh the items of the dynamic screen on top, keeping it open.
    pub fn replace_dynamic(&mut self, items: Vec<crate::providers::Item>) {
        let Some(builtin) = self.stack.last().and_then(|screen| match screen.source {
            Source::Dynamic { builtin, .. } => Some(builtin),
            _ => None,
        }) else {
            return;
        };
        let mut list = self.list_for(builtin, crate::providers::screen(builtin).layout, &items);
        let screen = self.screen_mut();
        if screen.list.mode() == pt35_ui::keys::Mode::Nav {
            list.keep_position(&screen.list);
        }
        screen.list = list;
        screen.source = Source::Dynamic { builtin, items };
    }

    /// Drop one row of the current dynamic screen, by payload.
    ///
    /// Used after a window is asked to close. Waiting for the daemon to notice
    /// would take a tree read that cannot see the future: sway answers `kill`
    /// before the client has gone.
    pub fn drop_dynamic(&mut self, payload: &str) {
        let Some(Source::Dynamic { items, .. }) = self.stack.last().map(|s| &s.source) else {
            return;
        };
        let items: Vec<crate::providers::Item> = items
            .iter()
            .filter(|item| item.payload != payload)
            .cloned()
            .collect();
        self.replace_dynamic(items);
    }

    /// Activate the nth row of the drawn window. Used by a tap.
    pub fn activate_window(&mut self, index: usize) -> Step {
        if !self.screen_mut().list.focus_window(index) {
            return Step::Nothing;
        }
        match self.screen().list.selected() {
            Some(item) => self.activate(item),
            None => Step::Nothing,
        }
    }

    pub fn handle(&mut self, key: &Key) -> Step {
        // X on the window picker is close-all, not search: a grid of three
        // tiles is not a list worth filtering, and closing everything had
        // nowhere else to live.
        if self.dynamic_builtin() == Some(Builtin::Windows)
            && pt35_ui::keys::navigate(key, self.screen().list.mode())
                == pt35_ui::keys::Navigation::StartFilter
        {
            return self.confirm_close_all();
        }
        // The picker is a strip: left and right move along it.
        if self.dynamic_builtin() == Some(Builtin::Windows) {
            let mode = self.screen().list.mode();
            use pt35_ui::keys::Button;
            let step = match pt35_ui::keys::button(key, mode) {
                Some(Button::Right) => Some(pt35_ui::keys::sym::DOWN),
                Some(Button::Left) => Some(pt35_ui::keys::sym::UP),
                _ => None,
            };
            if let Some(sym) = step {
                self.screen_mut().list.handle(&Key::new(sym));
                return Step::Redraw;
            }
        }
        let rows = self.rows;
        let outcome = self.screen_mut().list.handle(key);
        match outcome {
            Outcome::Redraw => Step::Redraw,
            Outcome::Nothing => Step::Nothing,
            Outcome::Cancel => Step::Quit,
            // Y jumps back to the root menu — on a handheld, backing out of
            // four levels one press at a time is the thing people complain about.
            Outcome::Secondary => {
                // On the launcher, Y pins the app under the cursor.
                if self.dynamic_builtin() == Some(Builtin::Launcher) {
                    return match self.selected_payload() {
                        Some(p) if p.starts_with("app:") || p.starts_with("exec:") => {
                            Step::TogglePin(p.to_string())
                        }
                        _ => Step::Nothing,
                    };
                }
                // On the switcher, Y closes the window under the cursor.
                if let Source::Dynamic {
                    builtin: Builtin::Windows,
                    items,
                } = &self.screen().source
                {
                    if let Some(payload) = self
                        .screen()
                        .list
                        .selected()
                        .and_then(|index| items.get(index))
                        .map(|item| &item.payload)
                        .cloned()
                    {
                        // The launcher is not a window and cannot be closed.
                        if payload == crate::providers::HOME {
                            return Step::Nothing;
                        }
                        return Step::Close(payload);
                    }
                }
                if self.stack.len() > 1 {
                    self.stack.truncate(1);
                    self.forget_entry();
                    Step::Redraw
                } else {
                    Step::Nothing
                }
            }
            Outcome::Back => {
                if self.back_closes() {
                    Step::Quit
                } else if self.stack.len() > 1 {
                    self.stack.pop();
                    self.forget_entry();
                    Step::Redraw
                } else if self.launcher_is_root() {
                    // The launcher is the home screen: nothing to go back to.
                    // You leave it by picking something, or with Start.
                    Step::Nothing
                } else {
                    Step::Quit
                }
            }
            Outcome::Activate(index) => {
                let _ = rows;
                self.activate(index)
            }
            // Left and Right change a quick setting in place and do nothing
            // anywhere else. Back is B, and a thumb resting on the D-pad must
            // not navigate.
            Outcome::Left | Outcome::Right => match self.focused_adjust() {
                Some(adjust) => Step::Adjust(adjust, matches!(outcome, Outcome::Right)),
                None => Step::Nothing,
            },
        }
    }

    fn activate(&mut self, index: usize) -> Step {
        let source = self.screen().source.clone();
        match source {
            Source::Confirm { command } => {
                // Item 0 is "No": back out instead of doing something drastic.
                if index == 0 {
                    self.stack.pop();
                    self.forget_entry();
                    Step::Redraw
                } else {
                    Step::Run(command)
                }
            }
            Source::Dynamic {
                builtin: Builtin::Launcher | Builtin::Quick,
                items,
            } => {
                let Some(payload) = items.get(index).map(|i| i.payload.clone()) else {
                    return Step::Nothing;
                };
                let (kind, rest) = payload.split_once(':').unwrap_or(("", payload.as_str()));
                let rest = rest.to_string();
                match kind {
                    "page" => {
                        self.push_page(&rest);
                        Step::Redraw
                    }
                    "screen" => match Builtin::from_name(&rest) {
                        Some(builtin) => Step::Open(builtin),
                        None => Step::Nothing,
                    },
                    "app" => Step::Run(Command::App(rest)),
                    "exec" => Step::Run(Command::Exec(rest)),
                    // A switch stays: watching it flip is the point.
                    "sh" => Step::RunStay(Command::Helper(rest)),
                    "ctl" => Step::RunStay(Command::Action(rest)),
                    // The D-pad is the whole interaction for a slider.
                    _ => Step::Nothing,
                }
            }
            Source::Dynamic { builtin, items } => match items.get(index).map(|i| &i.payload) {
                // A row you read. Running "nothing" would close the menu.
                Some(payload) if payload.is_empty() => Step::Nothing,
                Some(payload)
                    if builtin == Builtin::Windows && payload == crate::providers::HOME =>
                {
                    match self.go_home() {
                        Step::Nothing => Step::Redraw,
                        step => step,
                    }
                }
                // Only on screens of ours: a network could be called "sh:x".
                Some(payload)
                    if matches!(builtin, Builtin::Appearance | Builtin::Bluetooth)
                        && (payload.starts_with("theme:") || payload.starts_with("sh:")) =>
                {
                    let (kind, rest) = payload.split_once(':').unwrap_or_default();
                    Step::RunStay(match kind {
                        "theme" => Command::Theme(rest.to_string()),
                        _ => Command::Helper(rest.to_string()),
                    })
                }
                Some(payload) => Step::Run(Command::Dynamic {
                    builtin,
                    payload: payload.clone(),
                }),
                None => Step::Nothing,
            },
            Source::Page(id) => {
                let Some(page) = self.tree.page(&id) else {
                    return Step::Nothing;
                };
                let Some(entry) = page.entries.get(index) else {
                    return Step::Nothing;
                };
                let label = entry.label.clone();
                let confirm = entry.confirm;
                // A row that reads out its own state is a switch, and a switch
                // you cannot see flip is a guess.
                let stay = entry.state.is_some() || entry.stay;
                let kind = match entry.kind() {
                    Ok(kind) => kind,
                    Err(e) => {
                        log::warn!("{e}");
                        return Step::Nothing;
                    }
                };
                let command = match kind {
                    // The D-pad is the whole interaction for a quick setting.
                    Kind::Adjust => return Step::Nothing,
                    Kind::Goto(target) => {
                        self.push_page(&target);
                        return Step::Redraw;
                    }
                    Kind::Builtin(builtin) => return Step::Open(builtin),
                    Kind::App(app) => Command::App(app),
                    Kind::Exec(exec) => Command::Exec(exec),
                    Kind::Action(action) => Command::Action(action),
                };
                if confirm {
                    self.push_confirm(command, &label);
                    Step::Redraw
                } else if stay {
                    Step::RunStay(command)
                } else {
                    Step::Run(command)
                }
            }
        }
    }
}

/// Where the picker's cursor starts: the current window, or the first one
/// when none is current. The Launcher card at 0 only with no windows at all.
pub fn picker_start(items: &[crate::providers::Item]) -> usize {
    if items.len() <= 1 {
        return 0;
    }
    match items.iter().position(|item| item.active) {
        Some(at) if at >= 1 => at,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pt35_ui::keys::{sym, Key};

    const TREE: &str = r#"
root = "main"

[menu.main]
title = "PT35"
entries = [
  { label = "Applications", goto = "apps" },
  { label = "Windows", builtin = "windows" },
  { label = "Shut down", action = "power poweroff", confirm = true },
]

[menu.apps]
title = "Applications"
entries = [
  { label = "Terminal", app = "terminal" },
  { label = "Editor", app = "editor" },
]
"#;

    fn model() -> Model {
        Model::sized(toml::from_str(TREE).unwrap(), None, 9, 4, 3)
    }

    fn press(model: &mut Model, sym: u32) -> Step {
        model.handle(&Key::new(sym))
    }

    #[test]
    fn the_d_pad_sideways_does_not_navigate() {
        // Back is B. A thumb resting on the D-pad must not pop a screen or
        // open the row under the cursor.
        let mut model = model();
        assert_eq!(press(&mut model, sym::RETURN), Step::Redraw);
        assert_eq!(model.depth(), 2, "now one level down");
        assert_eq!(press(&mut model, sym::LEFT), Step::Nothing);
        assert_eq!(model.depth(), 2, "Left is not Back");
        assert_eq!(press(&mut model, sym::RIGHT), Step::Nothing);
        assert_eq!(model.depth(), 2, "Right is not Open");
        // B still is.
        assert_eq!(model.handle(&Key::with_text('b' as u32, 'b')), Step::Redraw);
        assert_eq!(model.depth(), 1);
    }

    #[test]
    fn opens_at_the_root() {
        let model = model();
        assert_eq!(model.screen().title, "PT35");
        assert_eq!(model.depth(), 1);
    }

    #[test]
    fn descends_into_a_submenu_and_back() {
        let mut model = model();
        assert_eq!(press(&mut model, sym::RETURN), Step::Redraw);
        assert_eq!(model.screen().title, "Applications");
        assert_eq!(press(&mut model, sym::BACKSPACE), Step::Redraw);
        assert_eq!(model.screen().title, "PT35");
        // Back at the root, Back leaves the menu entirely.
        assert_eq!(press(&mut model, sym::BACKSPACE), Step::Quit);
    }

    #[test]
    fn launching_an_app_returns_a_command() {
        let mut model = model();
        press(&mut model, sym::RETURN); // into Applications
        press(&mut model, sym::DOWN); // Editor
        assert_eq!(
            press(&mut model, sym::RETURN),
            Step::Run(Command::App("editor".into()))
        );
    }

    #[test]
    fn a_builtin_entry_asks_the_caller_to_populate_it() {
        let mut model = model();
        press(&mut model, sym::DOWN); // Windows
        assert_eq!(press(&mut model, sym::RETURN), Step::Open(Builtin::Windows));
    }

    #[test]
    fn destructive_entries_are_confirmed_and_default_to_no() {
        let mut model = model();
        press(&mut model, sym::DOWN);
        press(&mut model, sym::DOWN); // Shut down
        assert_eq!(press(&mut model, sym::RETURN), Step::Redraw);
        assert_eq!(model.screen().title, "Shut down?");
        assert_eq!(
            model.screen().list.selected(),
            Some(0),
            "the cursor starts on No"
        );

        // Choosing No backs out without running anything.
        assert_eq!(press(&mut model, sym::RETURN), Step::Redraw);
        assert_eq!(model.screen().title, "PT35");

        // Choosing Yes runs it.
        press(&mut model, sym::RETURN);
        press(&mut model, sym::DOWN);
        assert_eq!(
            press(&mut model, sym::RETURN),
            Step::Run(Command::Action("power poweroff".into()))
        );
    }

    #[test]
    fn submenu_rows_are_marked_for_the_chevron() {
        let model = model();
        let rows = model.visible_rows();
        assert_eq!(rows[0].label, "Applications");
        assert!(rows[0].submenu);
        assert!(rows[1].submenu);
        assert!(!rows[2].submenu, "an action row gets no chevron");
        assert_eq!(
            rows[0].glyph, "A",
            "a tile with no glyph falls back to the initial"
        );
    }

    #[test]
    fn y_jumps_back_to_the_root() {
        let mut model = model();
        press(&mut model, sym::RETURN); // into Applications
        assert_eq!(model.depth(), 2);
        assert_eq!(model.handle(&Key::with_text('y' as u32, 'y')), Step::Redraw);
        assert_eq!(model.depth(), 1);
        assert_eq!(model.screen().title, "PT35");
        // Already home: nothing to do.
        assert_eq!(
            model.handle(&Key::with_text('y' as u32, 'y')),
            Step::Nothing
        );
    }

    #[test]
    fn a_tap_activates_the_row_it_landed_on() {
        let mut model = model();
        assert_eq!(model.activate_window(1), Step::Open(Builtin::Windows));
        assert_eq!(model.activate_window(99), Step::Nothing);
    }

    #[test]
    fn the_d_pad_never_leaves_the_menu() {
        // A nudge on the D-pad must never close the menu.
        let mut model = model();
        assert_eq!(press(&mut model, sym::LEFT), Step::Nothing);
        assert_eq!(model.depth(), 1);
    }

    #[test]
    fn back_from_the_screen_you_came_in_on_leaves_the_menu() {
        let mut model = model();
        model.push_dynamic(Builtin::Windows, "Windows", Vec::new());
        model.mark_entry();
        assert_eq!(press(&mut model, sym::BACKSPACE), Step::Quit);

        // Walk above it and come back down: now Back is only Back.
        model.go_home();
        model.push_dynamic(Builtin::Windows, "Windows", Vec::new());
        assert_eq!(press(&mut model, sym::BACKSPACE), Step::Redraw);
        assert_eq!(model.depth(), 1);
    }

    #[test]
    fn the_picker_starts_on_the_window_you_are_in() {
        use crate::providers::Item;
        let item = |payload: &str, active: bool| Item {
            payload: payload.into(),
            active,
            ..Item::default()
        };
        let home = item(crate::providers::HOME, false);
        let three = [
            home.clone(),
            item("con:1", false),
            item("con:2", true),
            item("con:3", false),
        ];
        assert_eq!(picker_start(&three), 2);
        let none = [home.clone(), item("con:1", false), item("con:2", false)];
        assert_eq!(picker_start(&none), 1, "the first window, not the launcher");
        assert_eq!(picker_start(&[home]), 0);
    }

    #[test]
    fn escape_always_quits() {
        let mut model = model();
        press(&mut model, sym::RETURN);
        assert_eq!(press(&mut model, sym::ESCAPE), Step::Quit);
    }

    #[test]
    fn dynamic_screens_return_their_payload() {
        let mut model = model();
        model.push_dynamic(
            Builtin::Windows,
            "Windows",
            vec![
                crate::providers::Item::new("Launcher", crate::providers::HOME),
                crate::providers::Item::new("foot", "con:12"),
                crate::providers::Item::new("helix", "con:34"),
            ],
        );
        // Opens on the first window, past the Launcher row.
        press(&mut model, sym::DOWN);
        assert_eq!(
            press(&mut model, sym::RETURN),
            Step::Run(Command::Dynamic {
                builtin: Builtin::Windows,
                payload: "con:34".into()
            })
        );
    }

    #[test]
    fn a_missing_menu_page_shows_an_error_rather_than_panicking() {
        let tree: MenuTree = toml::from_str("root = \"nope\"\n[menu.main]\n").unwrap();
        let model = Model::sized(tree, None, 9, 4, 3);
        assert_eq!(model.screen().title, "missing menu");
        assert_eq!(model.depth(), 1);
    }
}
