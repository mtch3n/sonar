<img src="app/icons/app-icon.svg" width="96" alt="">

# Sonar

Sonar indexes your home folder and finds files as you type. It runs in the tray and opens its search bar when you press a shortcut. It also does arithmetic and unit conversions, and plugins from GitHub add more.

## Install

Download the latest build from [Releases](https://github.com/mtch3n/sonar/releases/latest).

| Platform | File |
|---|---|
| Linux | `.AppImage`, `.deb` or `.rpm` |
| macOS (Apple silicon and Intel) | `.dmg` |
| Windows | `.msi` or `-setup.exe` |
| Command line | `sonar-cli-*` for your platform |

The builds aren't code-signed yet. On macOS, right-click the app and choose Open the first time. On Windows, pick More info, then Run anyway.

## Shortcut

| Platform | Shortcut |
|---|---|
| macOS | ⌥ Space |
| Windows | Alt + Space |
| Linux, GNOME | Ctrl + Alt + Space, added to Settings → Keyboard → Custom Shortcuts |
| Other Linux desktops | Bind `sonar-app --toggle` to a key of your choice |

To use other keys, change `shortcut` in the [settings](#settings).

In the search bar, ↑ and ↓ move, Enter opens the file, Ctrl + Enter (⌘ + Enter on macOS) shows it in its folder, and Esc closes. The bottom of the bar shows what Enter and Ctrl + Enter do for the selected result.

## Search

Words match the start of words in file and folder names, so `inv` finds `invoice-march.pdf` and `photos` finds `backupPhotos.sh`. Every word has to match.

| Filter | Example | Meaning |
|---|---|---|
| `kind:` | `kind:pdf,image` | project, folder, app, code, script, key, pdf, doc, sheet, slides, image, video, audio, archive, config, other |
| `ext:` | `ext:sh` | file extension |
| `in:` | `in:~/Documents`, `in:notes` | under a path, or under any folder with that name |
| `name:` | `name:readme` | match the file name only |
| `modified:` | `modified:<30d`, `modified:>1y` | changed within, or longer ago than, a span (h, d, w, m, y) |
| `after:` `before:` | `after:2026-01-01` | changed on or after, or before, a date |
| `size:` | `size:>100mb`, `size:<10kb` | bigger or smaller than |
| `limit:` | `limit:50` | number of results, 20 by default |
| `-word` | `-draft` | leave out matches for a word |
| `"..."` | `"tax return"` | exact words |

A folder with `.git`, `Cargo.toml`, `package.json`, `pyproject.toml`, `go.mod` or similar counts as a project. It shows up as one result, and the files inside only appear when you use `kind:`, `ext:` or `in:`.

## Calculator

Type arithmetic or a unit conversion, like `2^10`, `sqrt 2` or `5 km to miles`, and the answer shows above your files. Enter copies it. Sonar only calculates when the query has a digit and doesn't look like a date, so file searches such as `invoice 2024` are left alone. The calculator is [fend](https://github.com/printfn/fend).

## Plugins

Plugins add results of their own. Most start with a keyword: with the Web search plugin, `g rust traits` offers to search Google, DuckDuckGo or GitHub.

Type `plugins` and a space to see what's installed and what the marketplaces offer. Enter installs or updates the selected plugin, and Ctrl + Enter removes it. Type `plugins` and a GitHub repository, like `plugins alice/sonar-emoji` or a github.com link, to add it: a repository with a single plugin is installed, and a marketplace is added to your list. Plugins… in the tray menu opens the same list.

A plugin is a program that runs with your permissions while the search bar is open, so install plugins from people you trust. Installing only downloads files; the plugin's program first runs when you search.

To write a plugin or run a marketplace, see [docs/plugins.md](docs/plugins.md).

## Settings

Settings… in the tray menu opens `settings.toml`, which Sonar writes on first run:

- Linux: `~/.config/sonar/settings.toml`
- macOS: `~/Library/Application Support/sonar/settings.toml`
- Windows: `%APPDATA%\sonar\settings.toml`

Sonar reads it again each time the search bar opens. If the file has a mistake, the bar says which line and Sonar keeps the last settings that worked.

| Setting | Default | Meaning |
|---|---|---|
| `shortcut` | `alt+space`, `ctrl+alt+space` on Linux | keys that open the search bar, like `ctrl+shift+k` |
| `marketplaces` | `["mtch3n/sonar"]` | GitHub repositories whose plugins you can install |
| `appearance.theme` | `system` | `system`, `light` or `dark` |
| `appearance.accent` | `#ff5a1f` | color of the selected icon and the text cursor |
| `appearance.width` | `720` | width of the search bar, 480 to 1600 |
| `appearance.rows` | `8` | results shown before the list scrolls, 3 to 20 |
| `search.limit` | `20` | results to find when the query has no `limit:` |
| `index.rescan_minutes` | `5` | how often to look for new and changed files |
| `updates.check` | `true` | look for new versions on GitHub |
| `plugins.<id>.enabled` | `true` | `false` turns a plugin off, including `calculator` |
| `plugins.<id>.keyword` | the plugin's own | another keyword for a plugin |

## What gets indexed

Your home folder, except hidden folders (`.ssh`, `.gnupg` and `.kube` are kept), whatever each folder's `.gitignore` leaves out, and the paths in the skip file. The skip file uses `.gitignore` syntax and is created on first run:

- Linux: `~/.config/sonar/ignore`
- macOS: `~/Library/Application Support/sonar/ignore`
- Windows: `%APPDATA%\sonar\ignore`

Sonar records names, sizes and dates, and the index never leaves your computer. Sonar itself only goes online to check GitHub for updates and, when you type `plugins`, to list and download plugins. The only time it opens a file is to read the first four bytes, when it needs to tell a script from a program or a Keynote deck from a key file.

The app rescans every five minutes, or as often as `index.rescan_minutes` says. To rescan now, use Reindex now in the tray menu or run `sonar index`.

## Updates

The app checks GitHub for a new release when it starts and every 12 hours after that. When one is out, the tray menu shows Update to vX.Y.Z. Click it, or click Check for updates at any time, and Sonar downloads the release, checks its signature, installs it and restarts. This works for the AppImage, `.deb`, `.rpm`, macOS and Windows installs. Set `updates.check = false` in the settings to stop the automatic checks.

The command-line tool updates itself with `sonar update`.

## Command line

```sh
sonar index
sonar s invoice kind:pdf
sonar s 'kind:image modified:<7d'
sonar s --help
sonar update
```

## Build from source

You need Rust (the version is pinned in `rust-toolchain.toml`), Node 24 or newer, and pnpm. On Linux you also need WebKitGTK 4.1 and libayatana-appindicator.

```sh
cd app
pnpm install
pnpm tauri dev
```

Release builds are signed so the updater can trust them. To build installers without the signing key, turn the update files off:

```sh
pnpm tauri build --config '{"bundle":{"createUpdaterArtifacts":false}}'
```

The command-line tool builds on its own with `cargo build --release -p sonar-cli`.

## Roadmap

- [Search inside files](https://github.com/mtch3n/sonar/issues/1)
- [Find duplicate and near-duplicate files](https://github.com/mtch3n/sonar/issues/2)
- [Summaries, embeddings and natural-language search](https://github.com/mtch3n/sonar/issues/3)
- [Labels and tags](https://github.com/mtch3n/sonar/issues/4)

## License

[MIT](LICENSE)
