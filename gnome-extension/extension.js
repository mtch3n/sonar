// Gives Sonar the open windows over D-Bus. On Wayland only the shell sees every
// window, so this is how a launcher can list them and switch to one.

import Gio from 'gi://Gio';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
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

export default class SonarWindows extends Extension {
    enable() {
        this._object = Gio.DBusExportedObject.wrapJSObject(INTERFACE, new Windows());
        this._object.export(Gio.DBus.session, PATH);
    }

    disable() {
        this._object?.unexport();
        this._object = null;
    }
}
