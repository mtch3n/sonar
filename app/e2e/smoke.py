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
        self.call("POST", f"/session/{self.session}/element/{self.element(css)}/click", {})

    def type(self, css: str, text: str):
        element = self.element(css)
        self.call("POST", f"/session/{self.session}/element/{element}/clear", {})
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
    """Types `query` and waits until a row satisfies `match`."""
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


def check_settings_window(driver: WebDriver, app: Path, env: dict, settings: Path):
    # Asking the running Sonar, as a desktop shortcut would, opens its Settings window.
    subprocess.run([str(app), "--settings"], env=env, timeout=15, check=True)

    def switch_to_settings():
        for handle in driver.call("GET", f"/session/{driver.session}/window/handles"):
            driver.call("POST", f"/session/{driver.session}/window", {"handle": handle})
            if driver.run("return location.hash") == "#settings":
                return handle

    wait_for(switch_to_settings, 15, "the Settings window")

    title = wait_for(
        lambda: driver.run("return document.querySelector('.titlebar h1')?.textContent"), 15, "the title bar"
    )
    assert title == "Settings", title
    buttons = driver.run("return [...document.querySelectorAll('.window-buttons button')].map(b => b.ariaLabel)")
    assert buttons == ["Minimize", "Close"], buttons
    print("ok  the window's own title bar")

    driver.click("button[aria-label='Browser settings']")
    shown = driver.run("return document.querySelector(\"button[aria-label='Search history']\").ariaChecked")
    assert shown == "true", shown
    driver.click("button[aria-label='Web search settings']")
    shown = driver.run("return document.querySelector(\"select[aria-label='Listed first']\").value")
    assert shown == "github", shown
    driver.click("button[aria-label='Calculator settings']")
    shown = driver.run("return document.querySelector(\"select[aria-label='Your currency']\").value")
    assert shown == "JPY", shown
    print("ok  settings form shows the saved values")

    driver.click("select[aria-label='Listed first'] option[value='duckduckgo']")
    driver.click("button[aria-label='Download exchange rates']")
    driver.click("select[aria-label='Code editor'] option:last-child")
    driver.type("input[aria-label='Code editor command']", "code --new-window")
    driver.click("button.primary")
    wait_for(lambda: driver.run("return document.querySelector('.status')?.textContent") == "Saved", 10, "Saved")
    text = settings.read_text()
    assert 'first = "duckduckgo"' in text and "rates = false" in text, text
    assert 'currency = "JPY"' in text, text
    assert '[files]\neditor = "code --new-window"' in text, text
    print("ok  saving writes plugin settings and the editor to settings.toml")

    driver.click("select[aria-label='Listed first'] option[value='google']")
    driver.click("button.primary")
    wait_for(lambda: 'first = "' not in settings.read_text(), 10, "the default to be left out")
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
