//! Open an installed app by typing part of its name, like `fire` for Firefox.

use sonar_apps::Launchable;

use crate::{Action, Item, Setting};

/// The apps plugin's id in `settings.toml`.
pub const ID: &str = "apps";

/// The most apps shown above the files.
const LIMIT: usize = 5;
/// Shorter queries match too many apps to be worth showing.
const MIN_QUERY: usize = 2;

pub fn settings() -> Vec<Setting> {
    Vec::new()
}

/// Runs the apps plugin, which `sonar-app --plugin apps` does. The list of apps is
/// read once, when the search bar opens.
pub fn serve() {
    let apps = sonar_apps::launchable();
    crate::serve(|query, _| Ok(answer(query, &apps)));
}

fn answer(query: &str, apps: &[Launchable]) -> Vec<Item> {
    let query = query.trim().to_lowercase();
    if query.chars().count() < MIN_QUERY {
        return Vec::new();
    }
    let mut found: Vec<(u8, &Launchable)> = apps
        .iter()
        .filter_map(|app| rank(&query, app).map(|rank| (rank, app)))
        .collect();
    found.sort_by_key(|(rank, app)| (*rank, app.name.len()));
    found
        .into_iter()
        .take(LIMIT)
        .map(|(_, app)| {
            let mut item = Item::new(app.name.clone(), Action::Run(app.open.clone()));
            item.subtitle = Some("App".into());
            item.image = app
                .icon
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned());
            // The glyph stands in if the picture can't be shown.
            item.icon = Some("app".into());
            item
        })
        .collect()
}

/// How well `query` names `app`, best first: the start of its name, the start of
/// words in its name, its initials, then other names and keywords.
fn rank(query: &str, app: &Launchable) -> Option<u8> {
    let name = app.name.to_lowercase();
    let words: Vec<&str> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let typed: Vec<&str> = query.split_whitespace().collect();
    let starts_words =
        |words: &[&str]| typed.iter().all(|t| words.iter().any(|w| w.starts_with(t)));
    if name.starts_with(query) {
        return Some(0);
    }
    if starts_words(&words) {
        return Some(1);
    }
    let initials: String = words.iter().filter_map(|w| w.chars().next()).collect();
    if typed.len() == 1 && initials.len() > 1 && initials.starts_with(query) {
        return Some(2);
    }
    let others = app.other_names.join(" ").to_lowercase();
    let other_words: Vec<&str> = others
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    starts_words(&other_words).then_some(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str, other: &[&str]) -> Launchable {
        Launchable {
            name: name.into(),
            other_names: other.iter().map(|o| o.to_string()).collect(),
            icon: None,
            open: vec![
                "gio".into(),
                "launch".into(),
                format!("/apps/{name}.desktop"),
            ],
        }
    }

    fn apps() -> Vec<Launchable> {
        vec![
            app("Firefox", &["Web Browser", "internet"]),
            app("Visual Studio Code", &["editor"]),
            app("Text Editor", &["notepad"]),
            app("Files", &["folder", "manager"]),
            app("Settings", &[]),
        ]
    }

    fn titles(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.title.as_str()).collect()
    }

    #[test]
    fn finds_apps_by_name_initials_and_other_names() {
        assert_eq!(titles(&answer("fire", &apps())), ["Firefox"]);
        assert_eq!(titles(&answer("text ed", &apps())), ["Text Editor"]);
        assert_eq!(titles(&answer("vsc", &apps())), ["Visual Studio Code"]);
        assert_eq!(titles(&answer("browser", &apps())), ["Firefox"]);
        assert_eq!(
            titles(&answer("ed", &apps())),
            ["Text Editor", "Visual Studio Code"],
            "names before keywords"
        );
        assert!(answer("f", &apps()).is_empty(), "too short");
        assert!(answer("invoice", &apps()).is_empty());
    }

    #[test]
    fn opens_the_app_with_its_icon() {
        let item = &answer("files", &apps())[0];
        assert_eq!(
            item.action,
            Action::Run(vec![
                "gio".into(),
                "launch".into(),
                "/apps/Files.desktop".into()
            ])
        );
        assert_eq!(
            item.icon.as_deref(),
            Some("app"),
            "the glyph when there's no icon file"
        );
    }
}
