import QtQuick
import Quickshell
import qs.Ui
import qs.Commons
import "BarText.js" as BarText

// The forecast wind at the boat in the bar, labeled Wind, in the theme's
// urgent color at small-craft advisory strength. The forecast, like
// "WSW 14G19 kn", is in the tooltip. Click for the hours ahead.
BarWidget {
    id: root
    moduleName: "org.omahoy.wind"

    // The shape the shell's summon, hide and popout switching expect.
    property bool opened: false
    property bool popoutSwitchClosing: false
    function open() {
        popoutSwitchClosing = false;
        opened = true;
    }
    function close() {
        opened = false;
    }
    function closeForPopoutSwitch() {
        popoutSwitchClosing = true;
        close();
    }

    readonly property var here: Wind.here
    readonly property string label: BarText.label()
    readonly property string detail: BarText.detail({
        incompatible: Wind.incompatible,
        connected: Wind.connected,
        waited: Wind.waited,
        summary: Wind.hasWind
            ? Wind.compass(here.dirDeg) + " " + Wind.knots(here.speedKn)
                + (Wind.gusty(here) ? "G" + Wind.knots(here.gustKn) : "") + " kn"
            : "",
        strong: Wind.strong,
        downloading: !!(Wind.fetch && Wind.fetch.status === "downloading")
    })

    implicitWidth: button.implicitWidth
    implicitHeight: button.implicitHeight

    WidgetButton {
        id: button
        anchors.fill: parent
        bar: root.bar
        text: root.label
        foreground: Wind.strong ? Color.urgent : (root.bar ? root.bar.barForeground : Color.foreground)
        // Faded when there's nothing current to show.
        dimmed: !Wind.connected || !Wind.hasWind || Wind.dated || (!!root.here && root.here.stale === true)
        tooltipText: root.detail
        onPressed: b => {
            if (b === Qt.LeftButton) {
                if (root.opened) root.close();
                else root.open();
            }
        }
    }

    KeyboardPanel {
        id: popup
        anchorItem: button
        bar: root.bar
        owner: root
        open: root.opened
        padding: 12
        borderSpec: Border.flat(Wind.strong ? Color.urgent : Color.accent, 2)
        // Fixed size: binding to the loaded list makes the popover jump as
        // it settles.
        contentWidth: 380
        contentHeight: 440
        focusTarget: content.item
        Loader {
            id: content
            anchors.fill: parent
            active: root.opened
            sourceComponent: Outlook {
                onCloseRequested: root.close()
            }
        }
    }
}
