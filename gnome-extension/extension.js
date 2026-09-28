// Gives Sonar what only GNOME Shell can see on Wayland, over D-Bus: the open
// windows, to list and switch to, and each thing copied, for its clipboard history.

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import St from 'gi://St';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const PATH = '/io/github/mtch3n/Sonar/Windows';
const INTERFACE = `<node>
  <interface name="io.github.mtch3n.Sonar.Windows">
    <method name="List"><arg type="s" direction="out" name="windows"/></method>
    <method name="Activate"><arg type="t" direction="in" name="id"/></method>
    <method name="Close"><arg type="t" direction="in" name="id"/></method>
  </interface>
</node>`;

const CLIPBOARD_PATH = '/io/github/mtch3n/Sonar/Clipboard';
const CLIPBOARD_INTERFACE = `<node>
  <interface name="io.github.mtch3n.Sonar.Clipboard">
    <signal name="Copied"><arg type="s" name="text"/></signal>
  </interface>
</node>`;

// Password managers like KeePassXC mark what they copy with this, to keep it out of
// clipboard histories.
const SECRET = 'x-kde-passwordManagerHint';

// Normal windows, most recently used first: the order Alt+Tab shows them in.
function windows() {
    return global.display.get_tab_list(Meta.TabList.NORMAL_ALL, null);
}

class Windows {
    List() {
        const tracker = Shell.WindowTracker.get_default();
        return JSON.stringify(windows().map(window => {
            const app = tracker.get_window_app(window);
            return {
                id: window.get_id(),
                title: window.get_title() ?? '',
                app: app?.get_name() ?? '',
                app_id: app?.get_id() ?? '',
                workspace: window.get_workspace()?.index() ?? -1,
                minimized: window.minimized,
            };
        }));
    }

    Activate(id) {
        const window = windows().find(w => w.get_id() === id);
        if (window)
            Main.activateWindow(window);
    }

    Close(id) {
        const window = windows().find(w => w.get_id() === id);
        if (window)
            window.delete(global.get_current_time());
    }
}

export default class Sonar extends Extension {
    enable() {
        this._object = Gio.DBusExportedObject.wrapJSObject(INTERFACE, new Windows());
        this._object.export(Gio.DBus.session, PATH);

        this._clipboard = Gio.DBusExportedObject.wrapJSObject(CLIPBOARD_INTERFACE, {});
        this._clipboard.export(Gio.DBus.session, CLIPBOARD_PATH);
        this._selection = global.display.get_selection();
        this._ownerChanged = this._selection.connect('owner-changed', (_, type) => {
            if (type === Meta.SelectionType.SELECTION_CLIPBOARD)
                this._copied();
        });
    }

    disable() {
        this._selection?.disconnect(this._ownerChanged);
        this._selection = null;
        this._clipboard?.unexport();
        this._clipboard = null;
        this._object?.unexport();
        this._object = null;
    }

    _copied() {
        const clipboard = St.Clipboard.get_default();
        if (clipboard.get_mimetypes(St.ClipboardType.CLIPBOARD).includes(SECRET))
            return;
        clipboard.get_text(St.ClipboardType.CLIPBOARD, (_, text) => {
            if (text?.trim())
                this._clipboard?.emit_signal('Copied', new GLib.Variant('(s)', [text]));
        });
    }
}
