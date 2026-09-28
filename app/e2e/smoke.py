"""End-to-end check of a built Sonar on Linux, through WebKit's WebDriver.

    python3 app/e2e/smoke.py target/release/bundle/appimage/Sonar_*.AppImage

Sonar runs on a headless mutter, on a D-Bus session of its own, so it never takes the
keyboard or talks to a Sonar you have open; add --visible to watch it on your screen
instead. It also gets a home folder of its own, so it never touches yours, and
downloads exchange rates, so it needs the network.

Needs WebKitWebDriver (Arch: webkitgtk-6.0, Debian and Ubuntu: webkit2gtk-driver),
mutter and dbus-run-session. Only the standard library is used.
"""

import json
import os
import shutil
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
PORT = 4445
DRIVER = f"http://127.0.0.1:{PORT}"


class WebDriver:
    def __init__(self, app: Path, env: dict, log, args=()):
        self.process = subprocess.Popen(
            ["WebKitWebDriver", f"--port={PORT}"],
            env=env,
            stdout=log,
            stderr=subprocess.STDOUT,
        )
        wait_for(lambda: port_open(PORT), 10, "WebKitWebDriver to start")
        answer = self.call(
            "POST",
            "/session",
            {"capabilities": {"alwaysMatch": {"webkitgtk:browserOptions": {"binary": str(app), "args": list(args)}}}},
        )
        self.session = answer["sessionId"]

    def call(self, method: str, path: str, body=None):
        data = None if body is None else json.dumps(body).encode()
        request = urllib.request.Request(
            DRIVER + path, data=data, method=method, headers={"Content-Type": "application/json"}
        )
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                return json.load(response)["value"]
        except urllib.error.HTTPError as err:
            raise RuntimeError(f"{method} {path}: {err.read().decode()}") from None

    def run(self, script: str, *args):
        return self.call("POST", f"/session/{self.session}/execute/sync", {"script": script, "args": list(args)})

    def element(self, css: str) -> str:
        """The first element matching `css`, waiting for the page to draw it."""
        body = {"using": "css selector", "value": css}
        found = wait_for(lambda: self.call("POST", f"/session/{self.session}/element", body), 15, css)
        return next(iter(found.values()))

    def click(self, css: str):
        element = self.element(css)
        # WebKit's driver doesn't scroll a list to what it clicks.
        self.run("document.querySelector(arguments[0]).scrollIntoView({block: 'center'})", css)
        self.call("POST", f"/session/{self.session}/element/{element}/click", {})

    def type(self, css: str, text: str):
        element = self.element(css)
        self.call("POST", f"/session/{self.session}/element/{element}/clear", {})
        self.call("POST", f"/session/{self.session}/element/{element}/value", {"text": text})

    def keys(self, css: str, text: str):
        """Sends keys to an element without emptying it first."""
        element = self.element(css)
        self.call("POST", f"/session/{self.session}/element/{element}/value", {"text": text})

    def quit(self):
        try:
            self.call("DELETE", f"/session/{self.session}")
        finally:
            self.process.terminate()
            self.process.wait(10)


def running_in(home: Path) -> list:
    """Processes started with `home` as their home folder: Sonar and its helpers."""
    marker = f"HOME={home}".encode()
    found = []
    for proc in Path("/proc").iterdir():
        try:
            if proc.name.isdigit() and marker in (proc / "environ").read_bytes().split(b"\0"):
                found.append(int(proc.name))
        except OSError:
            pass
    return found


def port_open(port: int) -> bool:
    with socket.socket() as s:
        return s.connect_ex(("127.0.0.1", port)) == 0


def wait_for(check, seconds: float, what: str):
    deadline = time.monotonic() + seconds
    last = None
    while time.monotonic() < deadline:
        try:
            last = check()
            if last:
                return last
        except RuntimeError as err:
            last = err
        time.sleep(0.2)
    raise AssertionError(f"waited {seconds}s for {what}; last saw {last!r}")


ROWS = """
return [...document.querySelectorAll('.row:not(.skeleton)')].map(row => ({
  title: row.querySelector('.row-title')?.textContent ?? '',
  subtitle: row.querySelector('.row-subtitle')?.textContent ?? '',
}));
"""


def search(driver: WebDriver, query: str, match, what: str):
    """Types `query` and waits until a row satisfies `match`. The box is emptied
    first, so rows left from the last query can't be mistaken for the answer."""
    driver.element("input[aria-label='Search']")
    driver.run(
        """const box = document.querySelector("input[aria-label='Search']");
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(box, "");
        box.dispatchEvent(new Event("input", { bubbles: true }));"""
    )
    wait_for(lambda: not driver.run(ROWS), 10, "the results to clear")
    driver.type("input[aria-label='Search']", query)
    return wait_for(
        lambda: next((row for row in driver.run(ROWS) if match(row)), None),
        30,
        f"{what} for {query!r}",
    )


def x_displays() -> set:
    return {p.name for p in Path("/tmp/.X11-unix").glob("X*")}


def headless_display() -> tuple[subprocess.Popen, dict]:
    """Starts mutter with a virtual monitor and no window on screen, and the
    variables that send Sonar to it and not to the desktop. It also runs an
    Xwayland of its own, because the AppImage starts GTK on X11."""
    name = f"sonar-e2e-{os.getpid()}"
    socket_path = Path(os.environ["XDG_RUNTIME_DIR"]) / name
    runtime = Path(os.environ["XDG_RUNTIME_DIR"])
    cookies = lambda: set(runtime.glob(".mutter-Xwaylandauth.*"))
    before, cookies_before = x_displays(), cookies()
    compositor = subprocess.Popen(
        ["mutter", "--headless", "--wayland", "--virtual-monitor", "1280x800", "--wayland-display", name],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    wait_for(socket_path.exists, 15, "the headless compositor")
    new = wait_for(lambda: x_displays() - before, 15, "the headless compositor's X display")
    display = ":" + min(new, key=lambda x: int(x[1:]))[1:]
    # Its X server only lets in programs with its own cookie, not the desktop's.
    cookie = wait_for(lambda: cookies() - cookies_before, 15, "the headless X server's cookie")
    return compositor, {
        "WAYLAND_DISPLAY": name,
        "DISPLAY": display,
        "XAUTHORITY": str(next(iter(cookie))),
        "GDK_BACKEND": None,
    }


def prepare(root: Path) -> dict:
    home = root / "home"
    files = {
        "scripts/backupPhotos.sh": "#!/bin/sh\n# nightly\nrsync -av ~/Pictures nas:/photos\n",
        "Documents/notes.md": "# Monday\n\nMeeting about the quarterly budget.\n",
    }
    for relative, text in files.items():
        path = home / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
    chrome = home / ".config" / "google-chrome" / "Default"
    chrome.mkdir(parents=True)
    (chrome / "Bookmarks").write_text(json.dumps({"roots": {"bookmark_bar": {"type": "folder", "children": [
        {"type": "url", "name": "Rust playground", "url": "https://play.rust-lang.org/"},
    ]}}}))
    history = sqlite3.connect(chrome / "History")
    history.execute(
        "CREATE TABLE urls(id INTEGER PRIMARY KEY, url TEXT, title TEXT, visit_count INTEGER NOT NULL DEFAULT 0,"
        " typed_count INTEGER NOT NULL DEFAULT 0, last_visit_time INTEGER NOT NULL, hidden INTEGER NOT NULL DEFAULT 0)"
    )
    history.execute("INSERT INTO urls (url, title, visit_count, last_visit_time) VALUES "
                    "('https://crates.io/crates/serde', 'serde - crates.io', 12, 0)")
    history.commit()
    history.close()
    applications = home / ".local" / "share" / "applications"
    applications.mkdir(parents=True)
    (applications / "sonar-test-pad.desktop").write_text(
        "[Desktop Entry]\nName=Sonar Test Pad\nKeywords=scratchpad;\nType=Application\nExec=true\n"
    )
    config = home / ".config" / "sonar"
    shutil.copytree(REPO / "plugins" / "web-search", config / "plugins" / "web-search")
    (config / "settings.toml").write_text(
        '[plugins.web-search]\nfirst = "github"\n\n[plugins.calculator]\ncurrency = "JPY"\n'
    )
    return dict(
        os.environ,
        HOME=str(home),
        XDG_CONFIG_HOME=str(home / ".config"),
        XDG_DATA_HOME=str(home / ".local" / "share"),
        XDG_CACHE_HOME=str(home / ".cache"),
        # Keeps Sonar from pointing the GNOME shortcut at this build.
        XDG_CURRENT_DESKTOP="none",
        TAURI_WEBVIEW_AUTOMATION="true",
    )


def check_search_window(driver: WebDriver):
    row = search(driver, "rsync", lambda r: r["title"] == "backupPhotos.sh", "the script found by its text")
    assert row["subtitle"] == "rsync -av ~/Pictures nas:/photos · ~/scripts", row
    print("ok  text inside files, with the matching line")

    row = search(driver, "photos", lambda r: r["title"] == "backupPhotos.sh", "the script found by its name")
    assert row["subtitle"] == "~/scripts", row
    print("ok  name matches show the folder only")

    search(driver, "g rust", lambda r: True, "Web search")
    first = driver.run(ROWS)[0]
    assert first["title"] == "Search GitHub for “rust”", first
    print("ok  plugin settings reach the plugin")

    row = search(driver, "100 usd to eur", lambda r: r["title"].endswith(" EUR"), "a currency conversion")
    assert row["subtitle"].startswith("ExchangeRate-API rates of "), row
    keys = driver.run("return document.querySelector('.footer .keys')?.textContent ?? ''")
    assert "Copy number" in keys, keys
    print(f"ok  currency conversion: {row['title']}, {row['subtitle']}")

    row = search(driver, "100 usd", lambda r: r["title"].endswith(" JPY"), "money in the home currency")
    print(f"ok  home currency from settings: {row['title']}")

    search(driver, "2^10", lambda r: r["title"] == "1024", "arithmetic")
    print("ok  arithmetic")

    row = search(driver, "playground", lambda r: r["title"] == "Rust playground", "a Chrome bookmark")
    assert row["subtitle"] == "play.rust-lang.org", row
    row = search(driver, "serde", lambda r: r["title"] == "serde - crates.io", "a page from Chrome's history")
    assert row["subtitle"] == "crates.io/crates/serde", row
    print("ok  Chrome bookmarks and history")

    search(driver, "sonar test", lambda r: r["title"] == "Sonar Test Pad", "an installed app")
    search(driver, "scratchpad", lambda r: r["title"] == "Sonar Test Pad", "an app by its keyword")
    print("ok  apps by name and keyword")

    search(driver, "slee", lambda r: r["title"] == "Sleep", "the System plugin")
    row = search(driver, "restart", lambda r: r["title"] == "Restart", "a command that asks first")
    assert row["subtitle"].startswith("Press Enter, then Enter again"), row
    print("ok  system commands, with restart asking first")

    row = search(driver, "kill mutter", lambda r: r["title"] == "mutter", "the Processes plugin")
    assert " · process " in row["subtitle"], row
    print("ok  processes listed by the kill keyword")


def check_live_index(driver: WebDriver, home: Path):
    """Files show up soon after they change, long before the five-minute rescan."""

    def appears(query: str, title: str):
        # Results don't refresh when the index does, so search again each time.
        def found():
            driver.type("input[aria-label='Search']", query)
            time.sleep(0.3)
            return any(row["title"] == title for row in driver.run(ROWS))

        wait_for(found, 15, f"{title} to show up for {query!r}")

    note = home / "Documents" / "quokka-sightings.txt"
    note.write_text("seen by the lake\n")
    appears("quokka", "quokka-sightings.txt")
    note.rename(note.with_name("wombat-sightings.txt"))
    appears("wombat", "wombat-sightings.txt")
    print("ok  new and renamed files show up within seconds")


CONTROL, NULL = "\ue009", "\ue000"


def check_actions(driver: WebDriver):
    """Ctrl + K lists a result's actions, and Read again reads the file again."""
    search(driver, "rsync", lambda r: r["title"] == "backupPhotos.sh", "the script")
    keys = driver.run("return document.querySelector('.footer .keys')?.textContent ?? ''")
    assert "Actions" in keys, keys
    driver.keys("input[aria-label='Search']", f"{CONTROL}k{NULL}")
    titles = wait_for(
        lambda: [row["title"] for row in driver.run(ROWS)] if driver.run(
            "return document.querySelector('.section-title')?.textContent ?? ''"
        ).startswith("Actions for") else None,
        10,
        "the actions list",
    )
    assert titles[-1] == "Read again", titles
    for _ in titles[1:]:
        driver.keys("input[aria-label='Search']", "\ue015")  # ↓
    driver.keys("input[aria-label='Search']", "\ue007")  # Enter
    notice = wait_for(
        lambda: driver.run("return document.querySelector('.notes')?.textContent ?? ''"), 10, "a notice"
    )
    assert notice == "Reading backupPhotos.sh again", notice
    search(driver, "rsync", lambda r: r["title"] == "backupPhotos.sh", "the script, read again")
    print("ok  Ctrl + K lists actions, and Read again reads a file again")


# The Settings window is built from shadcn controls: a select is a button that opens
# a list of options, and a switch is a button with aria-checked.
def trigger(label: str) -> str:
    return f"[data-slot='select-trigger'][aria-label='{label}']"


def choice(driver: WebDriver, label: str) -> str:
    value = f"{trigger(label)} [data-slot='select-value']"
    return driver.run(f"return document.querySelector(\"{value}\")?.textContent ?? ''")


def click_when_clear(driver: WebDriver, css: str):
    """Clicks once nothing covers the element, as a panel that's still unfolding can."""

    def attempt():
        try:
            driver.click(css)
            return True
        except RuntimeError as err:
            if "intercepted" not in str(err):
                raise
            return None

    try:
        wait_for(attempt, 10, f"{css} to be clickable")
    except AssertionError as err:
        cover = driver.run(
            """const r = document.querySelector(arguments[0]).getBoundingClientRect();
            return document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2)?.outerHTML.slice(0, 200);""",
            css,
        )
        raise AssertionError(f"{err}; covered by {cover}") from None


def choose(driver: WebDriver, label: str, option: str):
    click_when_clear(driver, trigger(label))
    # Options have no names of their own, so the one wanted is marked to be clicked.
    wait_for(
        lambda: driver.run(
            """document.querySelector('[data-e2e-pick]')?.removeAttribute('data-e2e-pick');
            const item = [...document.querySelectorAll("[data-slot='select-content'][data-open] [data-slot='select-item']")]
                .find(i => i.textContent === arguments[0]);
            item?.setAttribute('data-e2e-pick', '');
            return !!item;""",
            option,
        ),
        10,
        f"{option!r} in {label}",
    )
    click_when_clear(driver, "[data-e2e-pick]")
    wait_for(lambda: choice(driver, label) == option, 10, f"{label} to show {option!r}")
    # A scripted click picks the option but can leave the list open, as when the
    # option puts the focus elsewhere; Escape closes it.
    shown = "[...document.querySelectorAll(\"[data-slot='select-content']\")].filter(e => !e.parentElement.hidden)"
    escape = "document.dispatchEvent(new KeyboardEvent('keydown', {key: 'Escape', bubbles: true}))"
    wait_for(
        lambda: driver.run(
            f"""const shown = {shown};
            if (shown.some(e => !e.hasAttribute('data-closed'))) {escape};
            return shown.length === 0;"""
        ),
        10,
        f"the list of {label} to close",
    )


def press(driver: WebDriver, css: str):
    """Clicks through the page's own events: popups closing over the form can take a
    native click."""
    found = driver.run("const el = document.querySelector(arguments[0]); if (el) el.click(); return !!el;", css)
    assert found, f"nothing matches {css}"


def click_text(driver: WebDriver, tag: str, text: str):
    found = driver.run(
        f"""const el = [...document.querySelectorAll("{tag}")].find(e => e.textContent === arguments[0]);
        if (el) el.click();
        return !!el;""",
        text,
    )
    assert found, f"no {tag} saying {text!r}"


def save(driver: WebDriver):
    click_text(driver, "button", "Save")
    wait_for(
        lambda: driver.run("return document.querySelector('footer [role=status]')?.textContent") == "Saved",
        10,
        "Saved",
    )


def check_settings_window(driver: WebDriver, app: Path, env: dict, settings: Path):
    # Asking the running Sonar, as a desktop shortcut would, opens its Settings window.
    subprocess.run([str(app), "--settings"], env=env, timeout=15, check=True)

    def switch_to_settings():
        for handle in driver.call("GET", f"/session/{driver.session}/window/handles"):
            driver.call("POST", f"/session/{driver.session}/window", {"handle": handle})
            if driver.run("return location.hash") == "#settings":
                return handle

    wait_for(switch_to_settings, 15, "the Settings window")

    title = wait_for(lambda: driver.run("return document.querySelector('header h1')?.textContent"), 15, "the title bar")
    assert title == "Settings", title
    buttons = driver.run("return [...document.querySelectorAll('header button')].map(b => b.ariaLabel)")
    assert buttons == ["Minimize", "Close"], buttons
    print("ok  the window's own title bar")

    driver.click("button[aria-label='Browser settings']")
    shown = driver.run("return document.querySelector(\"[aria-label='Search history']\").ariaChecked")
    assert shown == "true", shown
    driver.click("button[aria-label='Web search settings']")
    assert choice(driver, "Listed first") == "GitHub", choice(driver, "Listed first")
    driver.click("button[aria-label='Calculator settings']")
    assert "JPY" in choice(driver, "Your currency"), choice(driver, "Your currency")
    print("ok  settings form shows the saved values")

    choose(driver, "Listed first", "DuckDuckGo")
    driver.click("[aria-label='Download exchange rates']")
    choose(driver, "Code editor", "Other command…")
    driver.type("input[aria-label='Code editor command']", "code --new-window")
    click_text(driver, "button", "Main screen")
    press(driver, "[role='switch'][aria-label='Search by meaning']")
    press(driver, "button[aria-label=\"What's indexed\"]")
    choose(driver, "Spreadsheets", "Name")
    save(driver)
    text = settings.read_text()
    assert 'first = "duckduckgo"' in text and "rates = false" in text, text
    assert 'currency = "JPY"' in text, text
    assert '[files]\neditor = "code --new-window"' in text, text
    assert 'monitor = "main"' in text, text
    assert "[meaning]\nenabled = true" in text, text
    assert '[index.kinds]\nsheet = "name"' in text, text
    print("ok  saving writes plugin settings, the editor, the screen, meaning and kinds to settings.toml")

    choose(driver, "Listed first", "Google")
    choose(driver, "Spreadsheets", "Name and words")
    press(driver, "[role='switch'][aria-label='Search by meaning']")
    save(driver)
    text = settings.read_text()
    assert 'first = "' not in text and "\n[index.kinds]" not in text, text
    print("ok  a value set back to its default is left out")


def main():
    visible = "--visible" in sys.argv
    args = [arg for arg in sys.argv[1:] if arg != "--visible"]
    if not visible and "SONAR_E2E_BUS" not in os.environ:
        # Start again on a private session bus, which the compositor and Sonar share.
        env = dict(os.environ, SONAR_E2E_BUS="1")
        os.execvpe("dbus-run-session", ["dbus-run-session", "--", sys.executable, __file__, *args], env)

    app = Path(args[0]).resolve()
    root = Path(tempfile.mkdtemp(prefix="sonar-e2e-"))
    env = prepare(root)
    compositor = None
    if not visible:
        compositor, display = headless_display()
        for key, value in display.items():
            if value is None:
                env.pop(key, None)
            else:
                env[key] = value
    log = open(root / "driver.log", "w")
    driver = WebDriver(app, env, log)
    try:
        check_search_window(driver)
        check_actions(driver)
        check_live_index(driver, root / "home")
        check_settings_window(driver, app, env, root / "home" / ".config" / "sonar" / "settings.toml")
    except BaseException:
        print(f"failed; Sonar's home folder and the driver log are in {root}")
        raise
    finally:
        driver.quit()
        # Sonar lives in the tray, so it outlasts the session by a moment.
        home = root / "home"
        wait_for(lambda: not running_in(home), 15, "Sonar to quit")
        if compositor:
            compositor.terminate()
            compositor.wait(10)
    shutil.rmtree(root)
    print("all end-to-end checks passed")


if __name__ == "__main__":
    main()
