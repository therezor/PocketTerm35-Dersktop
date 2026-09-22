//! The menu state machine: a stack of screens, each a filterable list.
//!
//! Deliberately free of Wayland and of any process spawning, so the whole
//! navigation model — including "what happens when you confirm a shutdown" —
//! is unit-testable.

use pt35_common::menu::{Builtin, Kind, MenuTree};
use pt35_ui::keys::Key;
use pt35_ui::list::{ListState, Outcome};

/// One screen on the stack.
#[derive(Debug, Clone)]
pub struct Screen {
    pub title: String,
    pub list: ListState,
    pub source: Source,
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
}

impl Model {
    /// Open `page` (or the tree's root when `None`).
    pub fn new(tree: MenuTree, page: Option<&str>, rows: usize) -> Self {
        let mut model = Self {
            tree,
            stack: Vec::new(),
            rows: rows.max(1),
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

    fn push_page(&mut self, id: &str) {
        let (title, labels) = match self.tree.page(id) {
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
            ),
            None => (
                "missing menu".to_string(),
                vec![format!("no menu {id:?} in menu.toml")],
            ),
        };
        let rows = self.rows;
        self.stack.push(Screen {
            title,
            list: ListState::new(labels, rows),
            source: Source::Page(id.to_string()),
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
        });
    }

    fn push_confirm(&mut self, command: Command, label: &str) {
        let rows = self.rows;
        self.stack.push(Screen {
            title: format!("{label}?"),
            list: ListState::new(vec!["No".into(), "Yes".into()], rows),
            source: Source::Confirm { command },
        });
    }

    pub fn handle(&mut self, key: &Key) -> Step {
        let rows = self.rows;
        let outcome = self.screen_mut().list.handle(key);
        match outcome {
            Outcome::Redraw => Step::Redraw,
            Outcome::Nothing => Step::Nothing,
            Outcome::Cancel => Step::Quit,
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
        Model::new(toml::from_str(TREE).unwrap(), None, 9)
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
        let model = Model::new(tree, None, 9);
        assert_eq!(model.screen().title, "missing menu");
        assert_eq!(model.depth(), 1);
    }
}
