pragma Singleton
import QtQuick
import Quickshell
import Quickshell.Io

// The connection to the wind engine, `omawind run`, shared by the bar on
// every monitor. The protocol is docs/protocol.md: newline-delimited JSON,
// version 1. When the engine isn't running this starts it, detached, so it
// keeps the forecast fresh whether or not the bar is looking.
QtObject {
    id: wind

    readonly property int version: 1
    readonly property string runtime: (Quickshell.env("XDG_RUNTIME_DIR") || "/tmp") + "/omawind/"
    readonly property string path: runtime + "wind.sock"
    // The checkout or plugin directory: this file is <repo>/ui/Wind.qml.
    readonly property string repo: decodeURIComponent(String(Qt.resolvedUrl("..")).replace(/^file:\/\//, "")).replace(/\/$/, "")
    readonly property string binary: Quickshell.env("OMAWIND_BIN") || repo + "/target/release/omawind"
    // The engine's stderr, so a failed start can be shown.
    readonly property string log: runtime + "engine.log"

    // The latest `state`; null while disconnected.
    property var state: null
    // The engine speaks another protocol version: stop, and say so.
    property bool incompatible: false
    // Unreachable for long enough that it's worth saying so.
    property bool waited: false
    property int attempts: 0
    property string lastLog: ""
    readonly property bool connected: socket !== null && socket.connected

    readonly property var forecast: state ? state.forecast : null
    readonly property var fetch: state && state.fetch ? state.fetch : null
    readonly property var here: state ? state.here : null
    // The hours that can be shown: a string time, a finite wind in range,
    // and gust and pressure only when they're numbers. A broken engine
    // can't break a row.
    readonly property var outlook: {
        if (!state || !Array.isArray(state.outlook)) return [];
        function num(v, lo, hi) { return typeof v === "number" && isFinite(v) && v >= lo && v <= hi; }
        return state.outlook.filter(h => h !== null && typeof h === "object" && typeof h.time === "string"
            && num(h.speedKn, 0, 250) && num(h.dirDeg, 0, 360)
            && (h.gustKn === undefined || num(h.gustKn, 0, 300))
            && (h.pressureHpa === undefined || num(h.pressureHpa, 800, 1100)));
    }
    readonly property var problems: state && Array.isArray(state.problems) ? state.problems : []
    readonly property bool hasWind: !!here && typeof here.speedKn === "number" && isFinite(here.speedKn)
        && typeof here.dirDeg === "number" && isFinite(here.dirDeg)
    // Sustained wind at small-craft advisory strength.
    readonly property bool strong: hasWind && here.speedKn >= 21
    // A forecast that should have been replaced by now, or has run out.
    readonly property bool dated: !!forecast && (forecast.status === "old" || forecast.status === "expired")

    // The latest `stations`, as far as it can be shown; null while
    // disconnected, and from an engine too old to send it.
    property var stations: null
    readonly property var measured: stations ? stations.stations : []

    function receive(line) {
        let m;
        try {
            m = JSON.parse(line);
        } catch (e) {
            return;
        }
        // Only a well-formed message counts. Anything else, even a bare {},
        // is ignored rather than taken as another protocol version.
        if (m === null || typeof m !== "object" || typeof m.v !== "number") return;
        // Lines already buffered after another version's are dropped too.
        if (wind.incompatible) return;
        if (m.v !== wind.version) {
            wind.incompatible = true;
            wind.state = null;
            wind.stations = null;
            wind.socket.connected = false;
            return;
        }
        if (m.type === "state" && m.forecast !== null && typeof m.forecast === "object"
                && m.here !== null && typeof m.here === "object")
            wind.state = m;
        else if (m.type === "stations" && Array.isArray(m.stations))
            wind.stations = wind.showable(m);
        // Other types are ignored, as the protocol asks.
    }

    // The stations that can be shown, so a broken engine can't break the
    // window: finite numbers in range, a direction left out only in a calm,
    // and text where text is read.
    function showable(m) {
        function num(v, lo, hi) { return typeof v === "number" && isFinite(v) && v >= lo && v <= hi; }
        const kept = m.stations.filter(s => s !== null && typeof s === "object"
            && typeof s.id === "string" && typeof s.time === "string"
            && (s.name === undefined || typeof s.name === "string")
            && num(s.lat, -90, 90) && num(s.lon, -180, 180) && num(s.speedKn, 0, 250)
            && (s.dirDeg === undefined ? s.speedKn === 0 : num(s.dirDeg, 0, 360))
            && (s.gustKn === undefined || num(s.gustKn, 0, 300)));
        return Object.assign({}, m, {stations: kept});
    }

    // Great-circle range in nautical miles, and the initial bearing in
    // degrees true, from one position to another.
    function rangeNm(lat1, lon1, lat2, lon2) {
        const r = Math.PI / 180;
        const a = Math.pow(Math.sin((lat2 - lat1) * r / 2), 2)
            + Math.cos(lat1 * r) * Math.cos(lat2 * r) * Math.pow(Math.sin((lon2 - lon1) * r / 2), 2);
        return 2 * 3440.065 * Math.asin(Math.min(1, Math.sqrt(a)));
    }
    function bearing(lat1, lon1, lat2, lon2) {
        const r = Math.PI / 180;
        const y = Math.sin((lon2 - lon1) * r) * Math.cos(lat2 * r);
        const x = Math.cos(lat1 * r) * Math.sin(lat2 * r) - Math.sin(lat1 * r) * Math.cos(lat2 * r) * Math.cos((lon2 - lon1) * r);
        return (Math.atan2(y, x) / r + 360) % 360;
    }

    // Sixteen points of where the wind blows from.
    function compass(deg) {
        const points = ["N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW", "NW", "NNW"];
        return points[Math.round((((deg % 360) + 360) % 360) / 22.5) % 16];
    }
    function knots(v) {
        return typeof v === "number" ? String(Math.round(v)) : "–";
    }
    // A gust worth mentioning: 3 knots or more over the wind.
    function gusty(h) {
        return typeof h.gustKn === "number" && typeof h.speedKn === "number" && h.gustKn - h.speedKn >= 3;
    }
    // A protocol time in local time: "14:00", or "Mon 14:00".
    function clock(iso, withDay) {
        const d = new Date(iso);
        return isNaN(d.getTime()) ? "" : Qt.formatDateTime(d, withDay ? "ddd HH:mm" : "HH:mm");
    }
    function age(minutes) {
        if (typeof minutes !== "number") return "";
        if (minutes < 60) return minutes + " min";
        const h = Math.floor(minutes / 60), m = minutes % 60;
        return h + " h" + (m ? " " + m + " min" : "");
    }

    // argv, never shell text built from paths. The engine keeps one copy
    // running through its lock file, so a second start is harmless.
    function start() {
        const script = 'mkdir -p -m 700 "$1" && b="$2"; [ -x "$b" ] || b=omawind; exec "$b" run 2>>"$3"';
        Quickshell.execDetached(["env", "-C", Quickshell.env("HOME") || "/", "sh", "-c", script,
                                 "omawind-start", wind.runtime, wind.binary, wind.log]);
    }

    property var socket: socketFactory.createObject(wind)
    property Component socketFactory: Component {
        Socket {
            path: wind.path
            connected: true
            parser: SplitParser {
                onRead: data => wind.receive(data)
            }
            onConnectedChanged: {
                if (connected) {
                    wind.attempts = 0;
                    wind.waited = false;
                } else {
                    wind.state = null;
                    wind.stations = null;
                }
            }
        }
    }

    // A failed connect leaves Quickshell's socket allocated, and toggling
    // `connected` can't retry it, so each retry is a fresh Socket. The
    // engine is started on the first failure and again every 20 tries.
    property Timer reconnect: Timer {
        interval: wind.attempts < 10 ? 1000 : 3000
        repeat: true
        running: wind.socket !== null && !wind.socket.connected && !wind.incompatible
        onTriggered: {
            wind.attempts += 1;
            if (wind.attempts % 20 === 1) wind.start();
            if (wind.attempts >= 6) wind.waited = true;
            // The log may not have existed when it was first watched.
            wind.logFile.reload();
            const previous = wind.socket;
            wind.socket = wind.socketFactory.createObject(wind);
            previous.destroy();
        }
    }

    property FileView logFile: FileView {
        path: wind.log
        watchChanges: true
        printErrors: false
        onFileChanged: reload()
        onLoaded: {
            const lines = text().trim().split("\n");
            wind.lastLog = lines.length ? lines[lines.length - 1] : "";
        }
    }
}
