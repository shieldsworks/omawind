import QtQuick
import qs.Commons

// The wind at the boat now, then hour by hour to the end of the run, with
// how old the forecast is. j and k scroll; Escape closes.
Item {
    id: root
    focus: true

    signal closeRequested
    Keys.onPressed: e => {
        if (e.key === Qt.Key_Escape) root.closeRequested();
        else if (e.key === Qt.Key_J || e.key === Qt.Key_Down) root.scroll(1);
        else if (e.key === Qt.Key_K || e.key === Qt.Key_Up) root.scroll(-1);
        else return;
        e.accepted = true;
    }
    function scroll(rows) {
        const most = Math.max(0, list.contentHeight - list.height);
        list.contentY = Math.max(0, Math.min(most, list.contentY + rows * 3 * 24));
    }

    readonly property color ink: Color.foreground
    // Secondary lines are the text color, faded. The theme's muted color is
    // too dark to read on the popover.
    readonly property real faint: 0.65
    readonly property string family: Style.font.family
    readonly property var here: Wind.here
    readonly property var forecast: Wind.forecast

    component Line: Text {
        width: parent.width
        color: root.ink
        font.family: root.family
        font.pixelSize: Style.font.caption
        elide: Text.ElideRight
    }

    Column {
        id: top
        anchors.left: parent.left
        anchors.right: parent.right
        spacing: 4

        Line {
            text: {
                if (Wind.incompatible) return "omawind speaks another protocol version";
                if (!Wind.connected) return Wind.waited ? "omawind isn't running" : "Starting omawind…";
                if (!root.here) return "Wind";
                if (root.here.at === "home") return "Wind at home";
                return root.here.stale ? "Wind where the boat was last seen" : "Wind at the boat";
            }
            font.pixelSize: Style.font.body
            font.bold: true
        }
        Line {
            visible: Wind.hasWind
            text: Wind.hasWind
                ? Wind.compass(root.here.dirDeg) + "  " + String(root.here.dirDeg).padStart(3, "0") + "°T   "
                  + Wind.knots(root.here.speedKn) + " kn"
                  + (typeof root.here.gustKn === "number" ? "   gusts " + Wind.knots(root.here.gustKn) : "")
                : ""
            color: Wind.strong ? Color.urgent : root.ink
            font.pixelSize: Style.font.body + 6
            font.bold: true
        }
        Line {
            visible: text !== ""
            text: {
                if (!Wind.connected) return Wind.waited && Wind.lastLog ? Wind.lastLog : "";
                if (!root.here) return "";
                if (!Wind.hasWind) return root.here.note || "";
                return typeof root.here.pressureHpa === "number" ? root.here.pressureHpa.toFixed(1) + " hPa at sea level" : "";
            }
            opacity: Wind.hasWind ? root.faint : 1
            wrapMode: Text.WordWrap
            maximumLineCount: 3
        }
        // Which run, how old, and what the engine is doing about it.
        Line {
            visible: text !== ""
            text: {
                const f = root.forecast;
                if (!f || f.status === "none") return "";
                let t = f.model + " run of " + Wind.clock(f.run, true) + " · " + Wind.age(f.ageMinutes) + " old";
                if (f.status === "old") t += " · no newer run yet";
                if (f.status === "expired") t += " · it has run out";
                return t;
            }
            color: root.forecast && root.forecast.status !== "ok" ? Color.urgent : root.ink
            opacity: root.forecast && root.forecast.status !== "ok" ? 1 : root.faint
        }
        Line {
            visible: text !== ""
            text: {
                const f = Wind.fetch;
                if (!f) return "";
                if (f.status === "downloading") return "Downloading hour " + (f.done + 1) + " of " + f.total + " from NOAA";
                if (f.status === "checking" && !(root.forecast && root.forecast.status === "ok")) return "Checking NOAA for a newer run";
                if (f.status === "error") return "Can't fetch from NOAA: " + (f.message || "unknown error");
                if (f.status === "off") return "Not fetching: omawind was started --offline";
                return "";
            }
            color: Wind.fetch && Wind.fetch.status === "error" ? Color.urgent : root.ink
            opacity: Wind.fetch && Wind.fetch.status === "error" ? 1 : root.faint
            wrapMode: Text.WordWrap
            maximumLineCount: 3
        }
    }

    Rectangle {
        id: rule
        anchors.top: top.bottom
        anchors.topMargin: 8
        anchors.left: parent.left
        anchors.right: parent.right
        height: 1
        color: root.ink
        opacity: 0.2
    }

    ListView {
        id: list
        anchors.top: rule.bottom
        anchors.topMargin: 6
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: foot.top
        anchors.bottomMargin: 6
        clip: true
        model: Wind.outlook
        delegate: Item {
            id: row
            required property var modelData
            required property int index
            readonly property var h: modelData
            // The day is named on the first hour and at each local midnight.
            readonly property bool newDay: index === 0 || Wind.clock(h.time, false) === "00:00"
            readonly property bool blowing: typeof h.speedKn === "number" && h.speedKn >= 21
            width: ListView.view.width
            height: 24

            component Cell: Text {
                anchors.verticalCenter: parent.verticalCenter
                color: row.blowing ? Color.urgent : root.ink
                font.family: root.family
                font.pixelSize: Style.font.body
            }

            Cell { x: 0; width: 84; text: Wind.clock(row.h.time, row.newDay); opacity: root.faint }
            // Points the way the wind blows.
            Cell { x: 88; width: 16; text: "↓"; rotation: row.h.dirDeg; horizontalAlignment: Text.AlignHCenter }
            Cell { x: 110; width: 40; text: Wind.compass(row.h.dirDeg) }
            Cell { x: 152; width: 60; horizontalAlignment: Text.AlignRight; text: Wind.knots(row.h.speedKn) + " kn"; font.bold: true }
            Cell { x: 218; width: 50; horizontalAlignment: Text.AlignRight; text: typeof row.h.gustKn === "number" ? "G" + Wind.knots(row.h.gustKn) : ""; opacity: root.faint }
            Cell { x: 276; width: parent.width - 276; horizontalAlignment: Text.AlignRight; text: typeof row.h.pressureHpa === "number" ? row.h.pressureHpa.toFixed(0) + " hPa" : ""; opacity: root.faint }
        }
    }

    Column {
        id: foot
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        spacing: 2
        Line {
            visible: Wind.problems.length > 0
            text: "⚠ " + Wind.problems.join(" · ")
            color: Color.urgent
            wrapMode: Text.WordWrap
            maximumLineCount: 3
        }
        Line {
            text: "Forecast, not observation. Not for navigation."
            opacity: root.faint
        }
    }
}
