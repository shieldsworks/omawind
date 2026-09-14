import QtQuick
import Quickshell
import Quickshell.Io

// The wind in a window of its own: at the boat now, every hour of the
// forecast, and what NOAA's stations are measuring, nearest the boat first.
// Run standalone (ui/shell.qml) it owns its process; as the shell's panel
// the shell opens and hides it. The bar's popover stays the quick look.
Item {
    id: app

    // Set by the Omarchy shell when loaded as a panel.
    property var shell: null
    property var manifest: null
    property bool standalone: true
    property bool opened: standalone

    function open(payload) {
        opened = true;
        Qt.callLater(() => surface.forceActiveFocus());
    }
    function close() {
        opened = false;
    }
    function dismiss() {
        if (standalone) Qt.quit();
        else if (shell) shell.hide("org.omahoy.wind");
        else opened = false;
    }

    property Theme theme: Theme {}

    // Quickshell keeps a process alive after its last window closes.
    Connections {
        target: Quickshell
        function onLastWindowClosed() { if (app.standalone) Qt.quit(); }
    }

    // Read by what shows an age, so it keeps counting.
    property real now: Date.now()
    Timer { interval: 30000; repeat: true; running: app.opened; onTriggered: app.now = Date.now() }

    readonly property var here: Wind.here
    readonly property var forecast: Wind.forecast
    readonly property bool placed: !!here && typeof here.lat === "number" && typeof here.lon === "number"
    readonly property string where: !here ? "" : here.at === "home" ? "home" : "the boat"

    // The stations, nearest the boat (or home) first, each with its range
    // and bearing from there.
    readonly property var stations: {
        const list = Wind.measured.map(s => app.placed
            ? {s: s, nm: Wind.rangeNm(app.here.lat, app.here.lon, s.lat, s.lon),
               brg: Wind.bearing(app.here.lat, app.here.lon, s.lat, s.lon)}
            : {s: s, nm: -1, brg: 0});
        list.sort((a, b) => app.placed ? a.nm - b.nm
            : String(a.s.name || a.s.id).localeCompare(String(b.s.name || b.s.id)));
        return list;
    }
    // Why there are no stations to list, or nothing.
    readonly property string stationsNote: {
        const m = Wind.stations;
        if (!Wind.connected || Wind.measured.length) return "";
        if (!m) return Wind.state ? "This omawind doesn't report stations: update it" : "";
        if (m.status === "off") return "Not asking NOAA: omawind was started --offline";
        if (m.status === "waiting") return "Asking NOAA's stations…";
        if (m.status === "error") return "Can't reach NOAA's stations: " + (m.message || "unknown error");
        return "No station in the region has reported wind in the last 2 hours";
    }

    function degrees(d) { return String(Math.round(d) % 360).padStart(3, "0"); }
    function nm(v) { return (v < 10 ? v.toFixed(1) : v.toFixed(0)) + " nm"; }
    // "SW 220°T  4 kn G6  ·  11:18, 12 min ago"
    function measuredText(s) {
        let t = typeof s.dirDeg !== "number" ? "calm"
            : Wind.compass(s.dirDeg) + " " + degrees(s.dirDeg) + "°T  " + Wind.knots(s.speedKn) + " kn"
              + (Wind.gusty(s) ? " G" + Wind.knots(s.gustKn) : "");
        const at = Date.parse(s.time);
        if (!isNaN(at))
            t += "  ·  " + Wind.clock(s.time, false) + ", " + Math.max(0, Math.round((app.now - at) / 60000)) + " min ago";
        return t;
    }

    function scroll(rows) {
        const most = Math.max(0, page.contentHeight - page.height);
        page.contentY = Math.max(0, Math.min(most, page.contentY + rows * 72));
    }

    function key(e) {
        if (e.key === Qt.Key_J || e.key === Qt.Key_Down) scroll(1);
        else if (e.key === Qt.Key_K || e.key === Qt.Key_Up) scroll(-1);
        else if (e.text === "g") page.contentY = 0;
        else if (e.text === "G") scroll(1e6);
        // A held key would flicker between palettes.
        else if (e.text === "n") { if (!e.isAutoRepeat) theme.night = !theme.night; }
        else if (e.text === "q" || e.key === Qt.Key_Escape) dismiss();
        else return;
        e.accepted = true;
    }

    // For checks, where no keyboard can be driven:
    //   quickshell ipc -p ui/shell.qml call omawind status
    IpcHandler {
        target: "omawind"
        function status(): string {
            return JSON.stringify({connected: Wind.connected, hasWind: Wind.hasWind, hours: Wind.outlook.length,
                                   stations: app.stations.length,
                                   nearest: app.stations.length ? app.stations[0].s.id : "",
                                   note: app.stationsNote, opened: app.opened, night: app.theme.night});
        }
        function night(): void { app.theme.night = !app.theme.night; }
        function scroll(rows: int): void { app.scroll(rows); }
    }

    FloatingWindow {
        id: win
        title: "Omawind"
        visible: app.opened
        onVisibleChanged: {
            if (!visible && app.opened) app.dismiss();
            else if (visible) Qt.callLater(() => surface.forceActiveFocus());
        }
        implicitWidth: Number(Quickshell.env("OMAWIND_WIDTH")) || 480
        implicitHeight: Number(Quickshell.env("OMAWIND_HEIGHT")) || 780
        color: app.theme.background

        // Plain text: labels carry what the engine sends, like a station's
        // name, and that mustn't be read as markup.
        component Label: Text {
            textFormat: Text.PlainText
            color: app.theme.foreground
            font.family: app.theme.font
            font.pixelSize: app.theme.baseSize
            elide: Text.ElideRight
        }
        component Heading: Label {
            width: parent.width
            color: app.theme.accent
            font.bold: true
            font.pixelSize: app.theme.baseSize - 1
        }
        component Rule: Item {
            width: parent.width
            height: 17
            Rectangle {
                anchors.verticalCenter: parent.verticalCenter
                width: parent.width
                height: 1
                color: app.theme.foreground
                opacity: 0.2
            }
        }

        Item {
            id: surface
            anchors.fill: parent
            focus: true
            Keys.onPressed: e => app.key(e)

            // The app's name, so the window is known at a glance even with
            // nothing to show.
            Label {
                id: appName
                anchors { left: parent.left; right: nightButton.left; top: parent.top; margins: 14 }
                text: "OMAWIND"
                color: app.theme.accent
                font.bold: true
                font.pixelSize: app.theme.baseSize - 1
            }

            // Night Watch for this window only: red on black.
            Rectangle {
                id: nightButton
                anchors { right: parent.right; rightMargin: 14; verticalCenter: appName.verticalCenter }
                height: 22
                width: nightLabel.implicitWidth + 16
                color: app.theme.night ? app.theme.accent : "transparent"
                border.width: 1
                border.color: app.theme.night ? app.theme.accent : Qt.alpha(app.theme.foreground, 0.25)
                Label {
                    id: nightLabel
                    anchors.centerIn: parent
                    text: "NIGHT  n"
                    color: app.theme.night ? app.theme.background : app.theme.foreground
                    font.pixelSize: app.theme.baseSize - 1
                }
                MouseArea { anchors.fill: parent; onClicked: app.theme.night = !app.theme.night }
            }

            Flickable {
                id: page
                anchors { left: parent.left; right: parent.right; top: appName.bottom; bottom: statusBar.top; topMargin: 10 }
                contentHeight: body.implicitHeight + 16
                clip: true
                boundsBehavior: Flickable.StopAtBounds

                Column {
                    id: body
                    x: 14
                    width: page.width - 28
                    spacing: 3

                    // ------------------------------------------------ now
                    Label {
                        width: parent.width
                        text: {
                            if (Wind.incompatible) return "omawind speaks another protocol version";
                            if (!Wind.connected) return Wind.waited ? "omawind isn't running" : "Starting omawind…";
                            if (!app.here) return "Wind";
                            if (app.here.at === "home") return "Wind at home";
                            return app.here.stale ? "Wind where the boat was last seen" : "Wind at the boat";
                        }
                        font.bold: true
                        font.pixelSize: app.theme.baseSize + 1
                    }
                    // Wind.here, not app.here: hasWind can turn true before
                    // app.here has caught up.
                    Label {
                        width: parent.width
                        visible: Wind.hasWind
                        text: {
                            const h = Wind.here;
                            if (!Wind.hasWind || !h) return "";
                            return Wind.compass(h.dirDeg) + "  " + app.degrees(h.dirDeg) + "°T   "
                                + Wind.knots(h.speedKn) + " kn"
                                + (typeof h.gustKn === "number" ? "   gusts " + Wind.knots(h.gustKn) : "");
                        }
                        color: Wind.strong ? app.theme.red : app.theme.foreground
                        font.pixelSize: app.theme.baseSize + 10
                        font.bold: true
                    }
                    Label {
                        width: parent.width
                        visible: text !== ""
                        text: {
                            if (!Wind.connected) return Wind.waited && Wind.lastLog ? Wind.lastLog : "";
                            if (!app.here) return "";
                            if (!Wind.hasWind) return app.here.note || "";
                            return typeof app.here.pressureHpa === "number" ? app.here.pressureHpa.toFixed(1) + " hPa at sea level" : "";
                        }
                        opacity: Wind.hasWind ? 0.65 : 1
                        wrapMode: Text.WordWrap
                        maximumLineCount: 3
                    }
                    // Which run, how old, and what the engine is doing about it.
                    Label {
                        width: parent.width
                        visible: text !== ""
                        text: {
                            const f = app.forecast;
                            if (!f || f.status === "none") return "";
                            let t = f.model + " run of " + Wind.clock(f.run, true) + " · " + Wind.age(f.ageMinutes) + " old";
                            if (f.status === "old") t += " · no newer run yet";
                            if (f.status === "expired") t += " · it has run out";
                            return t;
                        }
                        color: app.forecast && app.forecast.status !== "ok" ? app.theme.yellow : app.theme.foreground
                        opacity: app.forecast && app.forecast.status !== "ok" ? 1 : 0.65
                    }
                    Label {
                        width: parent.width
                        visible: text !== ""
                        text: {
                            const f = Wind.fetch;
                            if (!f) return "";
                            if (f.status === "downloading") return "Downloading hour " + (f.done + 1) + " of " + f.total + " from NOAA";
                            if (f.status === "checking" && !(app.forecast && app.forecast.status === "ok")) return "Checking NOAA for a newer run";
                            if (f.status === "error") return "Can't fetch from NOAA: " + (f.message || "unknown error");
                            if (f.status === "off") return "Not fetching: omawind was started --offline";
                            return "";
                        }
                        color: Wind.fetch && Wind.fetch.status === "error" ? app.theme.yellow : app.theme.foreground
                        opacity: Wind.fetch && Wind.fetch.status === "error" ? 1 : 0.65
                        wrapMode: Text.WordWrap
                        maximumLineCount: 3
                    }

                    Rule {}

                    // ------------------------------------------- forecast
                    Heading { text: "FORECAST" + (app.where ? " AT " + app.where.toUpperCase() : "") + ", HOUR BY HOUR" }
                    Label {
                        width: parent.width
                        visible: Wind.connected && Wind.outlook.length === 0
                        text: "No hours ahead in this forecast"
                        opacity: 0.65
                    }
                    Repeater {
                        model: Wind.outlook
                        delegate: Item {
                            id: hour
                            required property var modelData
                            required property int index
                            readonly property var h: modelData
                            // The day is named on the first hour and at each
                            // local midnight.
                            readonly property bool newDay: index === 0 || Wind.clock(h.time, false) === "00:00"
                            readonly property bool blowing: typeof h.speedKn === "number" && h.speedKn >= 21
                            readonly property real u: app.theme.baseSize
                            width: body.width
                            height: u + 11

                            component Cell: Label {
                                anchors.verticalCenter: parent.verticalCenter
                                color: hour.blowing ? app.theme.red : app.theme.foreground
                                elide: Text.ElideNone
                            }
                            Cell { x: 0; width: hour.u * 7.5; text: Wind.clock(hour.h.time, hour.newDay); opacity: 0.65 }
                            // Points the way the wind blows.
                            Cell { x: hour.u * 7.5; width: hour.u * 1.5; text: "↓"; rotation: hour.h.dirDeg; horizontalAlignment: Text.AlignHCenter }
                            Cell { x: hour.u * 9.5; width: hour.u * 3.5; text: Wind.compass(hour.h.dirDeg) }
                            Cell { x: hour.u * 13; width: hour.u * 5; horizontalAlignment: Text.AlignRight; text: Wind.knots(hour.h.speedKn) + " kn"; font.bold: true }
                            // A narrow row or a large font drops pressure,
                            // then the gust, rather than overlap them.
                            Cell { x: hour.u * 18.5; width: hour.u * 4; visible: hour.width >= hour.u * 23; horizontalAlignment: Text.AlignRight; text: typeof hour.h.gustKn === "number" ? "G" + Wind.knots(hour.h.gustKn) : ""; opacity: 0.65 }
                            Cell { anchors.right: parent.right; visible: hour.width >= hour.u * 31; horizontalAlignment: Text.AlignRight; text: typeof hour.h.pressureHpa === "number" ? hour.h.pressureHpa.toFixed(0) + " hPa" : ""; opacity: 0.65 }
                        }
                    }

                    Rule {}

                    // ------------------------------------------- measured
                    Heading {
                        text: {
                            const n = app.stations.length;
                            let t = "MEASURED NOW" + (n ? "  ·  " + n + (n === 1 ? " STATION" : " STATIONS") : "");
                            if (n && app.placed) t += ", NEAREST " + app.where.toUpperCase() + " FIRST";
                            return t;
                        }
                    }
                    Label {
                        width: parent.width
                        visible: Wind.stations && Wind.stations.status === "error" && Wind.measured.length > 0
                        text: "Can't reach NOAA just now: these are the last reports"
                        color: app.theme.yellow
                        wrapMode: Text.WordWrap
                    }
                    Label {
                        width: parent.width
                        visible: app.stationsNote !== ""
                        text: app.stationsNote
                        opacity: 0.65
                        wrapMode: Text.WordWrap
                        maximumLineCount: 3
                    }
                    Repeater {
                        model: app.stations
                        delegate: Item {
                            id: st
                            required property var modelData
                            readonly property var s: modelData.s
                            readonly property color tone: s.speedKn >= 21 ? app.theme.red : app.theme.foreground
                            width: body.width
                            height: lines.implicitHeight + 10
                            Column {
                                id: lines
                                y: 5
                                width: parent.width
                                spacing: 2
                                Item {
                                    width: parent.width
                                    height: stationName.implicitHeight
                                    Label {
                                        id: stationName
                                        anchors { left: parent.left; right: range.left; rightMargin: 12 }
                                        text: st.s.name || st.s.id
                                        color: st.tone
                                        font.bold: true
                                    }
                                    Label {
                                        id: range
                                        anchors.right: parent.right
                                        text: st.modelData.nm < 0 ? "" : app.nm(st.modelData.nm) + " " + app.degrees(st.modelData.brg) + "°"
                                        opacity: 0.65
                                    }
                                }
                                Label {
                                    width: parent.width
                                    text: app.measuredText(st.s)
                                    color: st.tone
                                }
                            }
                        }
                    }

                    Rule {}
                    Label {
                        width: parent.width
                        visible: Wind.problems.length > 0
                        text: "⚠ " + Wind.problems.join(" · ")
                        color: app.theme.yellow
                        wrapMode: Text.WordWrap
                        maximumLineCount: 4
                    }
                    Label {
                        width: parent.width
                        text: "A forecast is a model. A station measures where it stands, often on a pier. Look at the water."
                        opacity: 0.65
                        wrapMode: Text.WordWrap
                        font.pixelSize: app.theme.baseSize - 1
                    }
                }
            }

            // Sources | not for navigation.
            Rectangle {
                id: statusBar
                anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
                height: app.theme.baseSize + 16
                color: app.theme.background
                Rectangle { anchors { left: parent.left; right: parent.right; top: parent.top } height: 1; color: Qt.alpha(app.theme.foreground, 0.18) }
                Label {
                    anchors { left: parent.left; leftMargin: 14; verticalCenter: parent.verticalCenter }
                    width: parent.width - notNav.width - 42
                    text: "HRRR forecast  ·  NDBC stations"
                    opacity: 0.65
                }
                Label {
                    id: notNav
                    anchors { right: parent.right; rightMargin: 14; verticalCenter: parent.verticalCenter }
                    text: "Not for navigation"
                    opacity: 0.65
                }
            }
        }
    }

    Component.onCompleted: Qt.callLater(() => surface.forceActiveFocus())
}
