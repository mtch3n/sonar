# Writing Sonar plugins

A plugin is a folder with a `plugin.toml` and a program in any language. Sonar starts the program when the plugin is first searched, sends it each query on stdin, and shows the results it prints on stdout. The program keeps running while the search bar is open and is stopped when the bar closes.

[`plugins/web-search`](../plugins/web-search) is a complete plugin in under 40 lines of Python. Copy it to start your own.

## Try a plugin locally

Put the folder in Sonar's plugins folder and open the search bar. Sonar reads the folder again each time the bar opens, so edits apply the next time you open it.

- Linux: `~/.config/sonar/plugins/<id>/`
- macOS: `~/Library/Application Support/sonar/plugins/<id>/`
- Windows: `%APPDATA%\sonar\plugins\<id>\`

The folder name is the plugin's id. Settings refer to plugins by it, and it may only use lowercase letters, digits, `-` and `_`. If `plugin.toml` has a mistake, the search bar says what it is. Whatever your program writes to stderr goes to Sonar's log, prefixed with `sonar: plugin <id>:`.

## plugin.toml

```toml
name = "Web search"
description = "Search Google, DuckDuckGo or GitHub"
keyword = "g"
command = ["python3", "web_search.py"]
icon = "icon.svg"
```

| Key | Required | Meaning |
|---|---|---|
| `name` | yes | shown above the plugin's results and in the plugin list |
| `command` | yes | the program and its arguments, started in the plugin folder. A program with a folder in its path, like `bin/emoji`, is relative to the plugin folder; a bare name, like `python3`, is looked up on `PATH` |
| `description` | no | one line for the plugin list |
| `keyword` | no | one word. With a keyword, the plugin only gets queries that start with it and a space, and its results are the only ones shown. Without one, it gets every query and its results appear below the files |
| `icon` | no | a PNG, SVG, JPEG or WebP file in the plugin folder, up to 256 KB |

People can give your plugin another keyword, or turn it off, in their settings under `[plugins.<id>]`.

Keywords are the better choice for most plugins. A plugin without one runs on every keystroke and should only answer when it's confident, returning no items otherwise.

## The protocol

Sonar writes one JSON object per line to your program's stdin. For a plugin with a keyword, `query` is the text after the keyword, which may be empty.

```json
{"query": "rust traits"}
```

Your program answers each line with exactly one line of JSON on stdout, and flushes it:

```json
{"items": [{"title": "Search Google for “rust traits”", "subtitle": "google.com", "action": {"open": "https://www.google.com/search?q=rust+traits"}}]}
```

or, when something went wrong, a message Sonar shows in place of results:

```json
{"error": "Couldn't reach the server"}
```

Sonar sends the next query only after you've answered the last one, and skips queries the user has already typed past, so a slow plugin never falls behind. If you don't answer within 5 seconds, or print a line that isn't JSON, Sonar stops the program and starts it again for the next query. Unknown fields are ignored, so newer plugins keep working with older versions of Sonar.

### Items

| Field | Required | Meaning |
|---|---|---|
| `title` | yes | the main line |
| `subtitle` | no | a second, smaller line |
| `action` | yes | what Enter does |
| `alt` | no | what Ctrl + Enter (⌘ + Enter on macOS) does |

Sonar shows up to 50 items per answer.

### Actions

Sonar carries out actions itself, so plugins don't need platform-specific code to open a link or use the clipboard.

| Action | Example | What happens |
|---|---|---|
| `open` | `{"open": "https://example.com"}` | opens a URL, or a file or folder in its default app. A relative path is relative to the plugin folder |
| `reveal` | `{"reveal": "/home/me/notes.md"}` | shows a file or folder in the file manager |
| `copy` | `{"copy": "¯\\_(ツ)_/¯"}` | puts text on the clipboard |
| `run` | `{"run": ["notify-send", "Done"]}` | starts a program, with the rest of the list as its arguments. A program with a folder in its path is relative to the plugin folder |
| `fill` | `{"fill": "g rust "}` | replaces the search text, for example to complete a word |

Every action except `fill` closes the search bar.

## Publish a plugin

Push the plugin folder to a public GitHub repository, with `plugin.toml` at the root. People install it by typing `plugins` and the repository, like `plugins alice/sonar-emoji`. The plugin's id is the repository name without a `sonar-` prefix, so `alice/sonar-emoji` installs as `emoji`.

Sonar downloads the newest commit on the default branch. Enter on an installed plugin in the `plugins` list downloads it again.

Plugins run on every platform Sonar does only if their program does. A Python plugin needs Python; a compiled one needs a build for each platform, which the plugin can pick between with a small launcher script.

## Run a marketplace

A marketplace is a GitHub repository with a `marketplace.toml` at its root. People add it by typing `plugins` and the repository, and its plugins then show up in their `plugins` list. Sonar's own marketplace is [`marketplace.toml`](../marketplace.toml) in this repository.

```toml
name = "Alice's plugins"

# A plugin in a folder of this repository.
[[plugins]]
id = "clock"
name = "World clock"
description = "The time in other cities"
path = "plugins/clock"

# A plugin in a repository of its own.
[[plugins]]
id = "emoji"
name = "Emoji"
description = "Find an emoji by name and copy it"
repo = "alice/sonar-emoji"
```

Each plugin needs an `id` (lowercase letters, digits, `-` and `_`), a `name`, and either `path` or `repo`. The folder or repository must hold a `plugin.toml`.

To offer a plugin to everyone who uses Sonar, open a pull request that adds it to Sonar's `marketplace.toml`.
