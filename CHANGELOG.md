# Changelog

## [0.4.3] - 2026-09-29

Search:
- Search by meaning, off until you turn it on in Settings: `tenancy agreement for the apartment` finds a lease that never says those words. The model runs on your computer and is downloaded once: multilingual, which reads Chinese, or English. Results found by words and by meaning are ranked together, and every filter applies to both. A model behind an OpenAI-compatible API works too, as `provider:model`, like `ollama:bge-m3`
- Providers: openai, openrouter, ollama and lmstudio are known, and others can be added under Providers in Settings. API keys are kept in the system keychain, and folders listed as private are never sent to a model that isn't on your computer
- Everyday phrases work as filters: `pdfs from last week`, `photos in 2024`, `videos bigger than 1 gb`
- Tags: `tag:receipt` finds files tagged by rules (screenshots, a project's language), by Linux file tags, by Describe, by labels found by meaning or learned from files you tagged, and by hand. Tags you give follow a file's content wherever it goes. Labels are set under Labels in Settings
- Duplicates: `dupes:` groups copies, pictures and videos that look alike, and names like `report (1).pdf`, most space wasted first; `similar:` lists a file's look-alikes, and `sonar dupes` prints the groups
- Ctrl + K lists what a result can do: Tags…, Find similar and Read again

Indexing:
- Choose how much of each kind of file is indexed, under What's indexed in Settings: nothing, its name, its words, or its meaning too, and how much of each file's text is kept
- What's read from a file is remembered by its content, so moved, copied and touched files aren't read again, and a rebuilt index reads nothing it read before. Writes are noticed by the nanosecond and by the time programs can't set
- Older Excel files, `.xls` and `.xlsb`, are read too
- The index is rebuilt once

Plugins:
- Plugins can look at files as they're indexed, with a `[process]` table in `plugin.toml`
- Describe writes tags and a line about each picture and video, with a vision model you choose, to search them by
- Fingerprint makes perceptual hashes of pictures and videos, the way Stash does, to find look-alikes
- Top (`top`) shows what the computer is busy with: running programs as a tree, with their CPU and memory

Everything else:
- Choose the screen the search bar opens on: the one with the pointer, or the main one
- The search bar gets the keyboard when a shortcut opens it on GNOME
- Keys typed right after the search box is filled no longer land before the filled text
- Copying twice in a row no longer keeps a row of garbage in the clipboard history
- `sonar reindex`, `sonar tag` and `sonar dupes` are new on the command line
- Nightly builds include the Windows installer again

## [0.4.2] - 2026-09-28

- Clipboard history no longer needs Sonar's GNOME extension: Sonar watches the clipboard through XWayland, so it works on any Linux desktop from the first copy. The extension is back to listing windows only

## [0.4.1] - 2026-09-28

Plugins:
- Clipboard (`clip`, on GNOME) keeps the last 200 things you copied and finds text in them; Enter copies one again and Ctrl + Enter forgets it. Copies password managers mark as secret are skipped. Sonar's GNOME extension now also reports each copy, so it replaces a clipboard extension; installed copies of the extension offer to update
- Ports (`port`) shows which program listens on which port, and ends it
- DNS (`dns`) looks up a domain's records, or an address's name, from your DNS server, the one set in Settings, or one named with `@`
- Services (`svc`, on Linux) starts, stops, restarts, enables and disables systemd services, the system's and your own, and follows their logs in your terminal. systemd asks polkit, so the desktop's own password dialog appears when needed
- Plugins can say which systems they work on with `platforms`; elsewhere they aren't listed and can't be installed. Plugin results can run a command in your terminal

Settings:
- The Settings window is rebuilt with shadcn/ui: controls line up on the right, units sit inside their fields, and no native GTK widgets remain
- The accent color is picked from swatches like GNOME's, or System, which follows the desktop's accent; a custom color is typed in
- Update now, under the calculator, downloads exchange rates at once

Everything else:
- Apps and windows show their own icons even when the icon file is large, like Visual Studio Code's
- Nightly builds of main are published as a prerelease under the `nightly` tag, numbered for the next patch release; installed copies only update to releases

## [0.4.0] - 2026-09-28

Search:
- Words of three letters or more also match the text inside files outside code projects: notes, Markdown, CSV, scripts and config files, and PDF, Word, Excel, PowerPoint and OpenDocument files. The result shows the line that matched; name matches still come first. Keys and certificates are never read, nor OneDrive and iCloud files that aren't downloaded. The index is rebuilt once
- New, renamed, changed and deleted files show up about a second after they settle, instead of at the next rescan
- Bookmarks and history from Chrome, Chromium, Brave, Edge and Vivaldi (and Arc on macOS), from every profile, show above the files and open in the browser and profile they came from. Bookmarks, history and each profile can be turned off in Settings
- Installed apps: type part of an app's name, like `fire` or `vsc`, to open it

Calculator:
- Currencies: `100 usd to eur`, `$20 in yen`, and amounts like `100 usd` in your own currency, which starts as your region's. Ctrl + Enter copies just the number. Rates come from ExchangeRate-API once a day and are kept for offline use
- Time zones and dates: `time in tokyo`, `3pm tokyo in taipei`, `today + 90 days`, `days until 2026-12-25`

Plugins:
- Sonar's own features now run as plugins that can be turned off or given a keyword: Apps, Browser, Calculator, System (lock, sleep, restart, shut down, log out, empty trash; the drastic ones ask for a second Enter) and Processes (`kill chrome`)
- Windows (`w`) lists and switches between open windows on GNOME, through a small GNOME Shell extension Sonar offers to install. Bound to Alt + Tab with `sonar-app --query "w "`, it can stand in for GNOME's switcher
- Plugins can declare settings, which the Settings window shows under each plugin; put results above the files; give results their own icons and action names; and say which programs they need, like Python, with how to get them. Web search lets you pick the engine listed first

Everything else:
- Choose the editor that opens projects, code, scripts and config files, and the terminal that Ctrl + Enter opens folders and projects in; Settings lists the ones installed
- The Settings window has its own title bar, and both windows lose their hard borders
- The search bar no longer lags behind the keyboard on Linux
- `sonar-app --settings` opens Settings and `sonar-app --query "text"` opens the bar with text typed
- Saving from Settings adds sections an older settings.toml lacks as sections rather than inline tables

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
