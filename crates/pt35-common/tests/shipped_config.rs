//! The configuration we actually ship must parse and validate. This test is the
//! guard against config/code drift — edit `config/pt35/*.toml` and it runs here.

use pt35_common::{apps::AppTable, menu::Kind, menu::MenuTree, theme::Theme};
use std::path::PathBuf;

fn config(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../config/pt35")
        .join(name)
}

fn load<T: serde::de::DeserializeOwned>(name: &str) -> T {
    let path = config(name);
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} : {e}", path.display()));
    toml::from_str(&text).unwrap_or_else(|e| panic!("{} : {e}", path.display()))
}

#[test]
fn shipped_theme_parses() {
    let theme: Theme = load("theme.toml");
    assert!(
        theme.bar.height >= 12 && theme.bar.height <= 40,
        "bar must fit on a 480px screen"
    );
    assert!(theme.menu.row_height * theme.menu.rows_visible + theme.bar.height <= 480);
}

#[test]
fn shipped_menu_validates() {
    let tree: MenuTree = load("menu.toml");
    tree.validate().expect("shipped menu.toml is inconsistent");
}

#[test]
fn shipped_apps_validate() {
    let apps: AppTable = load("apps.toml");
    apps.validate().expect("shipped apps.toml is inconsistent");
}

#[test]
fn every_menu_app_entry_exists_in_apps_toml() {
    let tree: MenuTree = load("menu.toml");
    let apps: AppTable = load("apps.toml");
    for (id, page) in &tree.menus {
        for entry in &page.entries {
            if let Kind::App(app) = entry.kind().unwrap() {
                assert!(
                    apps.get(&app).is_some(),
                    "menu {id:?} entry {:?} launches unknown app {app:?}",
                    entry.label
                );
            }
        }
    }
}

#[test]
fn gui_apps_that_need_room_shrink_themselves() {
    // A big GTK/Qt app has to shrink its own UI, because the panel never does:
    // either a profile scale (which becomes GDK_DPI_SCALE / QT_SCALE_FACTOR) or
    // a scale flag of its own on the command line.
    let apps: AppTable = load("apps.toml");
    for id in ["browser", "files"] {
        let app = apps
            .get(id)
            .unwrap_or_else(|| panic!("missing app profile {id:?}"));
        assert!(
            !app.toolkit_env().is_empty() || app.exec.contains("scale-factor"),
            "{id} should shrink itself, got scale {} and exec {:?}",
            app.scale,
            app.exec
        );
    }
}
