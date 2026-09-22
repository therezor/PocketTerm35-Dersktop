//! The menu state machine: a stack of screens, each a filterable list.
//!
//! Deliberately free of Wayland and of any process spawning, so the whole
//! navigation model — including "what happens when you confirm a shutdown" —
//! is unit-testable.

use pt35_common::menu::{Builtin, Kind, Layout, MenuTree};
use pt35_common::theme::Rgb;
use pt35_ui::keys::Key;
use pt35_ui::list::{ListState, Outcome};

/// A row or tile as the UI draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub label: String,
    pub note: String,
    /// Set when the row opens a builtin screen, so the UI can replace `note`
    /// with live state.
    pub builtin: Option<Builtin>,
    pub glyph: String,
    pub tint: Option<Rgb>,
    pub submenu: bool,
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
    /// A dynamic list: the payloads are opaque strings the caller understands.
    Dynamic {
        builtin: Builtin,
        payloads: Vec<String>,
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
}

/// Result of feeding a key into the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Repaint.
    Redraw,
    /// Nothing changed.
    Nothing,
    /// The caller must populate this builtin screen and push it.
    Open(Builtin),
    /// Run this, then close the menu.
    Run(Command),
    /// Close the menu.
    Quit,
}

pub struct Model {
    tree: MenuTree,
    stack: Vec<Screen>,
    rows: usize,
    grid_rows: usize,
    columns: usize,
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
                Row {
                    label: label.to_string(),
                    note: entry.map(|e| e.note.clone()).unwrap_or_default(),
                    builtin: entry.and_then(|e| e.builtin),
                    glyph: entry
                        .map(|e| e.glyph.clone())
                        .filter(|g| !g.is_empty())
                        .unwrap_or_else(|| {
                            label
                                .chars()
                                .next()
                                .unwrap_or('?')
                                .to_uppercase()
                                .to_string()
                        }),
                    tint: entry.and_then(|e| e.tint),
                    submenu: self.leads_deeper(&screen.source, index),
                }
            })
            .collect()
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

    /// Push a screen built from live data (windows, networks, .desktop files).
    pub fn push_dynamic(&mut self, builtin: Builtin, title: &str, items: Vec<(String, String)>) {
        let (labels, payloads): (Vec<_>, Vec<_>) = items.into_iter().unzip();
        let rows = self.rows;
        self.stack.push(Screen {
            title: title.to_string(),
            list: ListState::new(labels, rows),
            source: Source::Dynamic { builtin, payloads },
            layout: Layout::List,
        });
    }

    fn push_confirm(&mut self, command: Command, label: &str) {
        let rows = self.rows;
        self.stack.push(Screen {
            title: format!("{label}?"),
            list: ListState::new(vec!["No".into(), "Yes".into()], rows),
            source: Source::Confirm { command },
            layout: Layout::List,
        });
    }

    pub fn handle(&mut self, key: &Key) -> Step {
        let rows = self.rows;
        let outcome = self.screen_mut().list.handle(key);
        match outcome {
            Outcome::Redraw => Step::Redraw,
            Outcome::Nothing => Step::Nothing,
            Outcome::Cancel => Step::Quit,
            // Y jumps back to the root menu — on a handheld, backing out of
            // four levels one press at a time is the thing people complain about.
            Outcome::Secondary => {
                if self.stack.len() > 1 {
                    self.stack.truncate(1);
                    Step::Redraw
                } else {
                    Step::Nothing
                }
            }
            Outcome::Back => {
                if self.stack.len() > 1 {
                    self.stack.pop();
                    Step::Redraw
                } else {
                    Step::Quit
                }
            }
            Outcome::Activate(index) => {
                let _ = rows;
                self.activate(index)
            }
        }
    }

    fn activate(&mut self, index: usize) -> Step {
        let source = self.screen().source.clone();
        match source {
            Source::Confirm { command } => {
                // Item 0 is "No": back out instead of doing something drastic.
                if index == 0 {
                    self.stack.pop();
                    Step::Redraw
                } else {
                    Step::Run(command)
                }
            }
            Source::Dynamic { builtin, payloads } => match payloads.get(index) {
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
                let kind = match entry.kind() {
                    Ok(kind) => kind,
                    Err(e) => {
                        log::warn!("{e}");
                        return Step::Nothing;
                    }
                };
                let command = match kind {
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
                } else {
                    Step::Run(command)
                }
            }
        }
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
                ("foot".into(), "con:12".into()),
                ("helix".into(), "con:34".into()),
            ],
        );
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
