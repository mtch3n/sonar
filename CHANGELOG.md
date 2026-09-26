# Changelog

## [0.3.0] - 2026-09-25

- A Settings window, opened with Settings… in the tray: the shortcut, theme, accent color, width, visible results, result limit, rescan interval, update checks, plugins and marketplaces. Saving applies the changes at once and keeps the comments in settings.toml
- Choosing the Plugins suggestion, or typing a plugin's keyword and a space, opens it. The space was dropped, so the same suggestion came back

## [0.2.2] - 2026-09-25

- The search bar is centered on one screen again. With several monitors side by side it could open across two, because a hidden window reported the wrong width

## [0.2.1] - 2026-09-25

- Settings… in the tray opens settings.toml again, and files and links open in their apps, from the AppImage too
- Python plugins such as Web search run from the AppImage; programs Sonar starts no longer inherit the AppImage's Python, GTK and library paths
- The search bar opens on the screen the pointer is on, and never straddles two screens

## [0.2.0] - 2026-09-25

- The search bar opens at the height of what it shows. On Linux it opened with empty space below the bar, because GTK kept the window at the web view's natural height
- Calculator and unit conversions, like `2^10` or `5 km to miles`; Enter copies the answer
- Plugins: programs in any language that add results, with or without a keyword. See docs/plugins.md
- Install, update and remove plugins by typing `plugins`, from marketplaces on GitHub or straight from a plugin's repository. Sonar's own marketplace starts with Web search
- `settings.toml` for the shortcut, theme, accent color, width, visible rows, result limit, rescan interval, update checks and plugins, read again each time the bar opens; Settings… in the tray opens it
- Results are grouped by where they come from, and the bottom of the bar shows what Enter and Ctrl + Enter do for the selected result
- Quieter selection: a neutral highlight with an accent-colored icon, in place of the orange edge
- The bar says when nothing matched, and when that may be because indexing isn't done

## [0.1.0] - 2026-09-25

First release.

- Search bar that opens with a keyboard shortcut and finds files as you type
- Tray icon that shows when indexing is running, with Reindex now and Quit
- Filters for kind, extension, folder, name, modified date, size and result count, plus `-word` to exclude and quotes for exact words
- Code projects show up as one result instead of every file inside them
- 16 kinds, including keys and certificates, scripts, apps, documents and images
- Chinese, Japanese and Korean file names match on part of the name
- `sonar` command-line tool for indexing and searching from a terminal
- Updates from GitHub Releases: the app checks on its own and installs from the tray menu, and `sonar update` updates the command-line tool
- Builds for Linux, macOS and Windows
