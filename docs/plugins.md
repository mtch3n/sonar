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

[[settings]]
key = "first"
title = "Listed first"
description = "The engine Enter searches"
type = "choice"
options = [
    { value = "google", title = "Google" },
    { value = "duckduckgo", title = "DuckDuckGo" },
]
```

| Key | Required | Meaning |
|---|---|---|
| `name` | yes | shown above the plugin's results and in the plugin list |
| `command` | yes | the program and its arguments, started in the plugin folder. A program with a folder in its path, like `bin/emoji`, is relative to the plugin folder; a bare name, like `python3`, is looked up on `PATH` |
| `description` | no | one line for the plugin list |
| `keyword` | no | one word. With a keyword, the plugin only gets queries that start with it and a space, and its results are the only ones shown. Without one, it gets every query and its results appear with the files |
| `position` | no | for a plugin without a keyword: `bottom`, the default, shows its results below the files; `top` shows them above, for short answers like a calculator's |
| `icon` | no | a PNG, SVG, JPEG or WebP file in the plugin folder, up to 256 KB |
| `settings` | no | options people can change; see [Settings](#settings) |
| `requires` | no | programs your plugin needs; see [Requirements](#requirements) |
| `platforms` | no | the systems your plugin works on: any of `linux`, `macos` and `windows`. Without it, it's offered everywhere; elsewhere it isn't listed and can't be installed |

People can give your plugin another keyword, or turn it off, in their settings under `[plugins.<id>]`.

Keywords are the better choice for most plugins. A plugin without one runs on every keystroke and should only answer when it's confident, returning no items otherwise.

## Settings

Each `[[settings]]` table declares one option. The Settings window lists them under your plugin, and Sonar keeps the values people choose in their `settings.toml`, next to the plugin's keyword:

```toml
[plugins.web-search]
first = "duckduckgo"
```

| Key | Required | Meaning |
|---|---|---|
| `key` | yes | the name in `settings.toml` and in the queries Sonar sends: lowercase letters, digits, `-` and `_`, but not `enabled` or `keyword` |
| `title` | yes | the label in the Settings window |
| `type` | yes | `text`, `number`, `toggle` or `choice` |
| `description` | no | a line under the label |
| `default` | no | the value until someone changes it. Without one, text is empty, a number is its `min` or 0, a toggle is off, and a choice is its first option |
| `placeholder` | no | for `text`: grey text shown while the field is empty |
| `min`, `max` | no | for `number`: the lowest and highest value allowed |
| `options` | for `choice` | a list of `{ value, title }`: `value` is what your plugin receives, `title` what people see |

Sonar checks values against what you declare, so your program always gets every key with a value of the right type. A value that doesn't fit is replaced by the default, and the search bar tells the person what to fix. Changes apply the next time the search bar opens.

Keep secrets such as API keys out of settings: `settings.toml` is often shared along with other dotfiles. Read them from an environment variable or your own file, or use a command-line tool that's already signed in, like `gh` for GitHub.

## Requirements

A plugin written in Python, Node or another language needs that language installed, and people may not have it. Say so, and tell them how to get it:

```toml
[[requires]]
program = "python3"
help = "Install Python 3 from https://www.python.org/downloads/ or your package manager."
```

`program` is looked up on `PATH`. While it's missing, Sonar doesn't start your plugin: the Settings window shows `help` under it, and typing its keyword shows `help` in place of results.

## The protocol

Sonar writes one JSON object per line to your program's stdin. For a plugin with a keyword, `query` is the text after the keyword, which may be empty. `settings` holds the value of every setting you declare, and is empty when you declare none.

```json
{"query": "rust traits", "settings": {"first": "google"}}
```

Your program answers each line with exactly one line of JSON on stdout, and flushes it:

```json
{"items": [{"title": "Search Google for “rust traits”", "subtitle": "google.com", "action": {"open": "https://www.google.com/search?q=rust+traits"}}]}
```

or, when something went wrong, a message Sonar shows in place of results:

```json
{"error": "Couldn't reach the server"}
```

Your program can keep files, like a cache, in the folder named by the `SONAR_PLUGIN_DATA` environment variable; Sonar creates it before starting your program.

Sonar sends the next query only after you've answered the last one, and skips queries the user has already typed past, so a slow plugin never falls behind. If you don't answer within 5 seconds, or print a line that isn't JSON, Sonar stops the program and starts it again for the next query. Unknown fields are ignored, so newer plugins keep working with older versions of Sonar.

### Items

| Field | Required | Meaning |
|---|---|---|
| `title` | yes | the main line |
| `subtitle` | no | a second, smaller line |
| `icon` | no | a glyph Sonar draws: `bookmark`, `history`, `window`, `process`, `port`, `service`, `power`, `lock`, `sleep`, `restart`, `logout`, `trash`, `terminal`, `clock`, `globe`, `clipboard`, `emoji`, `calculator`, `tag`, or a file kind like `folder`, `app` or `image`. Without one, the plugin's own icon is shown |
| `image` | no | a picture instead, like an app's icon: the path of an SVG up to 256 KB, or a PNG, JPEG or WebP up to 8 MB that Sonar scales down to fit, relative to the plugin folder or absolute, or a `data:image/` URL. Set `icon` too, for when the picture can't be read |
| `action` | yes | what Enter does |
| `alt` | no | what Ctrl + Enter (⌘ + Enter on macOS) does |
| `label`, `alt_label` | no | what the bottom of the bar calls Enter and Ctrl + Enter, when "Open" or "Copy" says too little, like `"Copy number"` |

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
| `terminal` | `{"terminal": ["journalctl", "-f"]}` | runs a program in the terminal chosen in Settings, with the rest of the list as its arguments |

Every action except `fill` closes the search bar.

## Processors

A plugin can also, or only, look at files as Sonar indexes them, and say what it learned: a line of text and tags that are then searched like the file's own words and by meaning, labels, or a perceptual fingerprint that finds look-alikes. Sonar's own Describe and Fingerprint plugins are processors. Add a `[process]` table to `plugin.toml`; a plugin that only processes needs no top-level `command`:

```toml
name = "Receipt reader"
description = "Totals and shops from photos of receipts"

[process]
kinds = ["image"]
command = ["python3", "read_receipt.py"]
version = "2"
model = "model"   # optional: the key of your setting that names a model as provider:model

[[settings]]
key = "model"
title = "Model"
type = "text"
default = "ollama:qwen2.5vl:3b"
```

| Key | Required | Meaning |
|---|---|---|
| `kinds` | yes | the kinds of file it's given, like `image`, `video` or `pdf`. Only files whose kind is set to `text` or `meaning` under What's indexed are |
| `command` | yes | the program that processes, and its arguments, found like `command` is |
| `version` | no | change it when your plugin would say something different, and Sonar gives it every file again. `1` by default |
| `frames` | no | frames of each video it wants, spread across the video, instead of the video itself. Sonar takes them with ffmpeg |
| `model` | no | the key of one of your settings that names a model as `provider:model`. Sonar sends that provider's address and key with each file, so your plugin never handles keys itself |

Sonar sends one line of JSON per file and waits for one line back, up to five minutes. Files are given once for each content: a copy, a moved file or one indexed again isn't sent again, unless `version` or the model changes.

```json
{"path": "/home/me/Pictures/IMG_0142.jpg", "kind": "image", "hash": "9f2c…", "frames": [], "duration": null,
 "settings": {"model": "ollama:qwen2.5vl:3b"},
 "model": {"url": "http://localhost:11434/v1", "key": null, "name": "qwen2.5vl:3b", "local": true},
 "labels": {"receipt": "proof of purchase listing items and a total"}}
```

For a video with `frames`, `frames` lists paths of JPEG frames and `duration` is its length in seconds. Answer with any of:

```json
{"text": "IKEA receipt for a desk, total 1,249 TWD", "tags": ["receipt", "ikea"], "labels": ["receipt"],
 "fingerprint": {"algo": "my-phash", "bits": "f0e1d2c3b4a59687"}}
```

or `{"error": "…"}`, which Sonar keeps so the file isn't tried again until `version` changes. `bits` is 64 bits as 16 hex digits; only fingerprints with the same `algo` are compared.

A processor whose model isn't on this computer is never given files from the folders the settings keep `private`. Sonar can't see what your program does with a file itself, though: if it sends files anywhere, say so in its description.

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
