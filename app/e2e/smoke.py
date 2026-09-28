"""End-to-end check of a built Sonar on Linux, through WebKit's WebDriver.

    python3 app/e2e/smoke.py target/release/bundle/appimage/Sonar_*.AppImage

Needs WebKitWebDriver (Arch: webkitgtk-6.0, Debian and Ubuntu: webkit2gtk-driver) and
a display. Sonar runs with a home folder of its own, so it never touches yours, and
downloads exchange rates, so it needs the network. Only the standard library is used.
"""

import json
import os
import shutil
import socket
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
    def __init__(self, app: Path, env: dict, log):
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
            {"capabilities": {"alwaysMatch": {"webkitgtk:browserOptions": {"binary": str(app), "args": []}}}},
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


def check_settings_window(driver: WebDriver, settings: Path):
    driver.run("location.hash = '#settings'; location.reload();")
    wait_for(lambda: driver.run("return !!document.querySelector('h1')"), 15, "the Settings window")

    driver.click("button[aria-label='Web search settings']")
    shown = driver.run("return document.querySelector(\"select[aria-label='Listed first']\").value")
    assert shown == "github", shown
    driver.click("button[aria-label='Calculator settings']")
    shown = driver.run("return document.querySelector(\"select[aria-label='Your currency']\").value")
    assert shown == "JPY", shown
    print("ok  settings form shows the saved values")

    driver.click("select[aria-label='Listed first'] option[value='duckduckgo']")
    driver.click("button[aria-label='Download exchange rates']")
    driver.click("button.primary")
    wait_for(lambda: driver.run("return document.querySelector('.status')?.textContent") == "Saved", 10, "Saved")
    text = settings.read_text()
    assert 'first = "duckduckgo"' in text and "rates = false" in text, text
    assert 'currency = "JPY"' in text, text
    print("ok  saving writes plugin settings to settings.toml")

    driver.click("select[aria-label='Listed first'] option[value='google']")
    driver.click("button.primary")
    wait_for(lambda: 'first = "' not in settings.read_text(), 10, "the default to be left out")
    print("ok  a value set back to its default is left out")


def main():
    app = Path(sys.argv[1]).resolve()
    root = Path(tempfile.mkdtemp(prefix="sonar-e2e-"))
    env = prepare(root)
    log = open(root / "driver.log", "w")
    driver = WebDriver(app, env, log)
    try:
        check_search_window(driver)
        check_settings_window(driver, root / "home" / ".config" / "sonar" / "settings.toml")
    except BaseException:
        print(f"failed; Sonar's home folder and the driver log are in {root}")
        raise
    finally:
        driver.quit()
    shutil.rmtree(root)
    print("all end-to-end checks passed")


if __name__ == "__main__":
    main()
