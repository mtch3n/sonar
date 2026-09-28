<img src="app/icons/app-icon.svg" width="96" alt="">

# Sonar

Sonar indexes your home folder and finds files, and the text inside them, as you type. It runs in the tray and opens its search bar when you press a shortcut. It also searches your browser's bookmarks and history, does arithmetic, unit and currency conversions, and plugins from GitHub add more.

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

`sonar-app --query "text"` opens the search bar with the text already typed, and `sonar-app --settings` opens the Settings window.

To use other keys, change `shortcut` in the [settings](#settings).

In the search bar, ↑ and ↓ move, Enter opens the file, Ctrl + Enter (⌘ + Enter on macOS) shows it in its folder, or opens a folder or project in your terminal, and Esc closes. The bottom of the bar shows what Enter and Ctrl + Enter do for the selected result.

## Search

Words match the start of words in file and folder names, so `inv` finds `invoice-march.pdf` and `photos` finds `backupPhotos.sh`. Every word has to match.

Words of three letters or more also match the text inside files outside code projects: notes, Markdown, CSV, scripts and config files, PDFs, Word, Excel and PowerPoint files (`.docx`, `.xlsx`, `.pptx`), and OpenDocument files (`.odt`, `.ods`, `.odp`). `rsync` finds a backup script that runs rsync, `boiler` finds the lease that mentions it, and the result shows the line that matched: a paragraph, a slide's line of text or a spreadsheet row. Name matches come first. Sonar keeps the first 64 KB of text from each file, or as much as `index.text_kb` says, skips documents over 20 MB and ones that are encrypted or broken, and never reads keys and certificates or files that are only stored in OneDrive or iCloud. Older `.doc`, `.xls` and `.ppt` files are found by name only.

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

Type arithmetic or a unit conversion, like `2^10`, `sqrt 2` or `5 km to miles`, and the answer shows above your files. Enter copies it. Sonar only does arithmetic when the query has a digit and doesn't look like a date, so file searches such as `invoice 2024` are left alone. The calculator is [fend](https://github.com/printfn/fend).

It converts currencies too: `100 usd to eur`, `$20 in yen`. Money without a target, like `100 usd` or `10 eur + 5 usd`, is shown in your own currency, which Sonar takes from your system's region and you can change in Settings. Ctrl + Enter copies just the number. Exchange rates come from [ExchangeRate-API](https://www.exchangerate-api.com) once a day and are kept for when you're offline; turn off Download exchange rates under the calculator in Settings to stop that.

It knows time zones and dates as well:

- `time in tokyo`, `tokyo time` or `now in london` shows the time there and how far ahead or behind you it is.
- `3pm tokyo in taipei`, `15:30 utc to pst` or `9am new york to berlin` converts a time between places. Leave a place out and it's yours: `3pm tokyo` is that time in your zone, and `3pm in tokyo` is your 3pm there.
- `today + 90 days`, `today + 3 weeks` or `2026-10-01 + 45 days` gives the date and its weekday, and `days until 2026-12-25` or `2026-12-25 - today` counts the days.

Places are the cities time zones are named after, like Tokyo or New York, some other common names, like Beijing, Delhi, NYC or San Francisco, and abbreviations like UTC, GMT, EST, PST, CET, IST and JST. An abbreviation stands for one place and follows its daylight saving time, so `pst` in summer is shown as PDT; CST is US Central and IST is India. Enter copies the answer, and Ctrl + Enter copies it as `2026-09-28T14:00+08:00`, `2026-12-27` or a plain number of days.

## Browser

Bookmarks and history from Chrome, Chromium, Brave, Edge and Vivaldi, and Arc on macOS, show above your files, from every profile. Enter opens the page in the browser and profile it came from, and Ctrl + Enter copies its address. When you use several profiles, each result says which one it's from.

Sonar reads the browsers' own files, so it needs no extension, and nothing is sent anywhere. Under Browser in Settings you can turn bookmarks, history or any single profile off, change how many results show, give it a keyword like `b` so it only answers `b rust docs`, or turn it off.

## Plugins

Plugins add results of their own. Sonar ships with these, which can be turned off or given another keyword in Settings like any other plugin:

- Apps: type part of an app's name, like `fire` for Firefox or `vsc` for Visual Studio Code, to open it
- Calculator and Browser, described above
- System: type `lock`, `sleep`, `restart`, `shut down`, `log out` or `empty trash`; restarting, shutting down and logging out ask for a second Enter first
- Processes: `kill chrome` lists matching programs by memory; Enter ends one, Ctrl + Enter forces it
- Ports: `port 3000` or `port node` shows which program listens on which port, and whether other computers can reach it; Enter ends the program, Ctrl + Enter forces it
- DNS: `dns example.com` looks up its addresses, `dns example.com mx` or `txt`, `ns`, `cname`, `soa`, `srv`, `caa` other records, and `dns 1.1.1.1` an address's name. It asks your system's DNS server, or the one set under DNS in Settings; `@9.9.9.9` in the search asks another. Enter copies a record
- Services, on Linux: `svc docker` lists systemd services, running ones first, the system's and your own; Enter starts or stops one and Ctrl + Enter follows its logs in your terminal. A word first does just that: `svc restart docker`, `svc enable sshd`, `svc disable cups`, `svc logs nginx`. For the system's services, systemd asks for your password through the desktop's usual dialog
- Windows, on GNOME: `w` lists your open windows, most recent first; Enter switches to one, Ctrl + Enter closes it
- Clipboard, on Linux: `clip` lists what you copied, newest first, and finds text in it: `clip invoice`. Enter copies it again, Ctrl + Enter removes it from the history. Sonar keeps the last 200 copies of text, and skips passwords that password managers like KeePassXC mark as secret

The clipboard history can stand in for a clipboard extension such as Clipboard Indicator: bind Super + V to `sonar-app --query "clip "` in Settings → Keyboard → Custom Shortcuts. It needs no extension: Sonar watches the copy of the clipboard that Wayland desktops keep for X11 apps (XWayland), which works in GNOME and KDE alike.

GNOME only shows other apps' windows to its own extensions, so the first time you type `w`, Sonar offers to install a small one; log out and back in afterwards.

To use the window list in place of Alt + Tab, move GNOME's switcher to another key and bind Alt + Tab to Sonar with the window list already typed:

```sh
gsettings set org.gnome.desktop.wm.keybindings switch-applications "['<Super>Tab']"
```

Then add a custom shortcut in Settings → Keyboard with the command `sonar-app --query "w "` (the AppImage's path in place of `sonar-app` if you use it). Unlike GNOME's switcher, it stays open when you let go of Alt: type to narrow the list and press Enter. Most plugins start with a keyword: with the Web search plugin, `g rust traits` offers to search Google, DuckDuckGo or GitHub.

Type `plugins` and a space to see what's installed and what the marketplaces offer. Enter installs or updates the selected plugin, and Ctrl + Enter removes it. Type `plugins` and a GitHub repository, like `plugins alice/sonar-emoji` or a github.com link, to add it: a repository with a single plugin is installed, and a marketplace is added to your list. Plugins… in the tray menu opens the same list.

A plugin is a program that runs with your permissions while the search bar is open, so install plugins from people you trust. Installing only downloads files; the plugin's program first runs when you search.

To write a plugin or run a marketplace, see [docs/plugins.md](docs/plugins.md).

## Settings

Settings… in the tray menu, or `sonar-app --settings`, opens the Settings window. Everything in it is saved to `settings.toml`, which you can also edit by hand; Sonar writes it on first run:

- Linux: `~/.config/sonar/settings.toml`
- macOS: `~/Library/Application Support/sonar/settings.toml`
- Windows: `%APPDATA%\sonar\settings.toml`

Sonar reads it again each time the search bar opens. If the file has a mistake, the bar says which line and Sonar keeps the last settings that worked.

| Setting | Default | Meaning |
|---|---|---|
| `shortcut` | `alt+space`, `ctrl+alt+space` on Linux | keys that open the search bar, like `ctrl+shift+k` |
| `marketplaces` | `["mtch3n/sonar"]` | GitHub repositories whose plugins you can install |
| `appearance.theme` | `system` | `system`, `light` or `dark` |
| `appearance.accent` | `#ff5a1f` | color of the selected icon and the text cursor, or `system` for the desktop's accent color |
| `appearance.width` | `720` | width of the search bar, 480 to 1600 |
| `appearance.rows` | `8` | results shown before the list scrolls, 3 to 20 |
| `search.limit` | `20` | results to find when the query has no `limit:` |
| `files.editor` | empty | the command that opens projects, code, scripts and config files, like `code`, `zed` or `open -a 'Visual Studio Code'`; empty opens them in their default app. Settings lists the editors it finds |
| `files.terminal` | empty | the terminal Ctrl + Enter opens folders and projects in, like `ptyxis` or `open -a iTerm`; empty uses the first one found. Settings lists the terminals it finds |
| `index.rescan_minutes` | `5` | how often to rescan everything, for changes the watch missed |
| `index.text_kb` | `64` | how much of each file's text is searched, 1 to 16384; changing it reads every file again |
| `updates.check` | `true` | look for new versions on GitHub |
| `plugins.<id>.enabled` | `true` | `false` turns a plugin off, including `calculator` |
| `plugins.calculator.currency` | your region's | the currency amounts like `100 usd` are shown in, like `"EUR"` |
| `plugins.calculator.rates` | `true` | download exchange rates once a day |
| `plugins.browser.bookmarks` | `true` | search bookmarks |
| `plugins.browser.history` | `true` | search history |
| `plugins.browser.<browser>-<profile>` | `true` | `false` leaves one profile out, like `chrome-profile-7`; Settings lists every profile it finds |
| `plugins.browser.results` | `3` | browser results shown above your files, 1 to 10 |
| `plugins.<id>.keyword` | the plugin's own | another keyword for a plugin |
| `plugins.<id>.<setting>` | the plugin's own | a plugin's own settings, like `first = "duckduckgo"` for Web search. The Settings window lists them under each plugin |

## What gets indexed

Your home folder, except hidden folders (`.ssh`, `.gnupg` and `.kube` are kept), whatever each folder's `.gitignore` leaves out, and the paths in the skip file. The skip file uses `.gitignore` syntax and is created on first run:

- Linux: `~/.config/sonar/ignore`
- macOS: `~/Library/Application Support/sonar/ignore`
- Windows: `%APPDATA%\sonar\ignore`

Sonar records names, sizes and dates, and the first 64 KB of text (or `index.text_kb`) from plain-text files, PDFs, and Office and OpenDocument files outside code projects, for searching inside them; it never reads keys and certificates. It also opens a file's first four bytes when it needs to tell a script from a program or a Keynote deck from a key file, and reads your browsers' bookmarks and history when you search. The index never leaves your computer.

Sonar itself only goes online to check GitHub for updates, to download exchange rates once a day (turn off Download exchange rates under the calculator in Settings to stop that) and, when you type `plugins`, to list and download plugins.

The app watches the folders it indexes and picks up new, renamed, changed and deleted files a second or so after they settle; a folder that never stops changing is rescanned at least every ten seconds. On Linux it watches each indexed folder, so a home with more folders than `fs.inotify.max_user_watches` allows stops watching and says so on the terminal. Either way it also rescans everything every five minutes, or as often as `index.rescan_minutes` says, to catch anything the watch missed. To rescan now, use Reindex now in the tray menu or run `sonar index`.

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

`cargo test --workspace` and, in `app`, `pnpm test` run the tests. On Linux, `python3 app/e2e/smoke.py target/release/sonar-app` also drives a release build through WebKit's WebDriver: it searches, converts currencies and saves plugin settings, with a home folder of its own. It runs on a headless mutter, so it never takes your keyboard; add `--visible` to watch it. It needs `WebKitWebDriver` (`webkit2gtk-driver` on Debian and Ubuntu), mutter and the network. AppImages built on a rolling distribution like Arch can crash at start; build them on Ubuntu 22.04, as the release workflow does.

## Roadmap

- [Search inside files](https://github.com/mtch3n/sonar/issues/1)
- [Find duplicate and near-duplicate files](https://github.com/mtch3n/sonar/issues/2)
- [Summaries, embeddings and natural-language search](https://github.com/mtch3n/sonar/issues/3)
- [Labels and tags](https://github.com/mtch3n/sonar/issues/4)

## License

[MIT](LICENSE)
