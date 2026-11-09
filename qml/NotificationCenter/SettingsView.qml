import QtQuick
import Nyx.Ui

// Notification settings: Do not disturb (also on a schedule), how long history is kept,
// deleting it, codes, pop-up text (when Nyx shows pop-ups), and per app: pop-ups and
// history.
Column {
    id: s

    required property var client
    property bool dnd: false
    property bool nyx: false
    property bool shown: false
    property real maxHeight: 700
    property var settings: null
    property var apps: []
    property string error: ""

    signal back

    readonly property var times: {
        let t = [];
        for (let i = 0; i < 48; i++)
            t.push((i < 20 ? "0" : "") + Math.floor(i / 2) + ":" + (i % 2 ? "30" : "00"));
        return t;
    }

    function read() {
        client.request("GetSettings", {}, (ok, err) => {
            error = err ? err.msg : "";
            if (ok)
                settings = ok;
        });
        client.request("Apps", {}, ok => apps = ok || []);
    }
    onShownChanged: if (shown)
        read()

    function change(f) {
        const next = JSON.parse(JSON.stringify(settings));
        f(next);
        settings = next;
        client.request("SetSettings", { settings: next }, ok => {
            if (ok)
                settings = ok;
        });
    }
    function listed(list, app) {
        return (list || []).indexOf(app) >= 0;
    }
    function setListed(key, app, on) {
        change(n => {
            n[key] = (n[key] || []).filter(x => x !== app);
            if (on)
                n[key].push(app);
        });
    }

    spacing: 0

    Item {
        width: parent.width
        height: 40
        Row {
            anchors.verticalCenter: parent.verticalCenter
            spacing: 6
            IconButton {
                icon: "go-previous-symbolic"
                tooltip: "Back"
                onClicked: s.back()
            }
            Label {
                anchors.verticalCenter: parent.verticalCenter
                text: "Notifications"
                role: "title"
                font.weight: Font.DemiBold
            }
        }
    }
    Item {
        width: 1
        height: 10
    }

    Flickable {
        width: parent.width
        height: Math.min(body.implicitHeight, s.maxHeight)
        contentHeight: body.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds

        Column {
            id: body
            width: parent.width
            spacing: 0

            Empty {
                visible: s.error !== ""
                width: parent.width
                icon: "dialog-warning-symbolic"
                text: s.error
            }

            Column {
                visible: s.settings !== null
                width: parent.width
                spacing: 0

                SettingsCard {
                    width: parent.width
                    OptionRow {
                        title: "Do not disturb"
                        sub: "Pop-ups stay quiet; everything still arrives here"
                        Toggle {
                            checked: s.dnd
                            onToggled: on => {
                                if (on !== s.dnd)
                                    s.client.request("ToggleDnd", {}, null);
                            }
                        }
                    }
                    OptionRow {
                        title: "On a schedule"
                        extra: Row {
                            spacing: 6
                            enabled: !!(s.settings && s.settings.schedule.enabled)
                            opacity: enabled ? 1 : 0.5
                            Choice {
                                pill: false
                                items: s.times
                                menuWidth: 110
                                index: s.settings ? Math.min(47, Math.floor(s.settings.schedule.from / 30)) : 0
                                onChosen: i => s.change(n => n.schedule.from = i * 30)
                            }
                            Label {
                                anchors.verticalCenter: parent.verticalCenter
                                text: "to"
                                size: 12.5
                                color: Style.dimText
                            }
                            Choice {
                                pill: false
                                items: s.times
                                menuWidth: 110
                                index: s.settings ? Math.min(47, Math.floor(s.settings.schedule.to / 30)) : 0
                                onChosen: i => s.change(n => n.schedule.to = i * 30)
                            }
                        }
                        Toggle {
                            checked: !!(s.settings && s.settings.schedule.enabled)
                            onToggled: on => s.change(n => n.schedule.enabled = on)
                        }
                    }
                }

                Section {
                    title: "History"
                    leftPadding: 8
                    topPadding: 14
                    bottomPadding: 6
                }
                SettingsCard {
                    width: parent.width
                    OptionRow {
                        title: "Keep for"
                        sub: "Pinned notifications are always kept"
                        Segmented {
                            width: 3 * 56 + 8
                            options: [{ id: "7", label: "7 d" }, { id: "30", label: "30 d" }, { id: "90", label: "90 d" }]
                            current: s.settings ? String(s.settings.retention_days) : "30"
                            onChosen: id => s.change(n => n.retention_days = parseInt(id))
                        }
                    }
                    OptionRow {
                        title: "Delete history"
                        sub: "Everything except pinned"
                        Button {
                            text: "Delete"
                            danger: true
                            armedText: "Delete everything"
                            onClicked: s.client.request("DeleteHistory", {}, null)
                        }
                    }
                    OptionRow {
                        title: "Keep verification codes"
                        sub: "Off: codes in history are hidden once ten minutes old"
                        Toggle {
                            checked: !!(s.settings && s.settings.keep_codes)
                            onToggled: on => s.change(n => n.keep_codes = on)
                        }
                    }
                }

                // Pop-ups: only when Nyx shows them (Omarchy has its own otherwise).
                Section {
                    visible: s.nyx
                    title: "Pop-ups"
                    leftPadding: 8
                    topPadding: 14
                    bottomPadding: 6
                }
                SettingsCard {
                    visible: s.nyx
                    width: parent.width
                    OptionRow {
                        title: "Show text in pop-ups"
                        sub: "Off: only the app and the title"
                        Toggle {
                            checked: !!(s.settings && !s.settings.hide_popup_text)
                            onToggled: on => s.change(n => n.hide_popup_text = !on)
                        }
                    }
                }

                // Apps: pop-ups (when Nyx shows them) and history, each its own switch.
                Item {
                    visible: s.apps.length > 0
                    width: parent.width
                    height: appsHead.implicitHeight
                    Section {
                        id: appsHead
                        title: "Apps"
                        leftPadding: 8
                        topPadding: 14
                        bottomPadding: 6
                    }
                    Row {
                        anchors.right: parent.right
                        anchors.rightMargin: 14
                        anchors.bottom: parent.bottom
                        anchors.bottomMargin: 6
                        spacing: 10
                        Repeater {
                            model: s.nyx ? ["Pop-ups", "History"] : ["History"]
                            delegate: Label {
                                required property var modelData
                                width: 48
                                horizontalAlignment: Text.AlignHCenter
                                text: modelData.toUpperCase()
                                size: 10
                                font.weight: Font.DemiBold
                                font.letterSpacing: 0.4
                                color: Style.tertiary
                            }
                        }
                    }
                }
                SettingsCard {
                    visible: s.apps.length > 0
                    width: parent.width
                    Repeater {
                        model: s.apps
                        delegate: Item {
                            id: appRow
                            required property var modelData
                            width: parent.width
                            height: 40
                            Row {
                                x: 8
                                anchors.verticalCenter: parent.verticalCenter
                                spacing: 10
                                AppIcon {
                                    anchors.verticalCenter: parent.verticalCenter
                                    icon: appRow.modelData.app_icon || ""
                                    size: 20
                                }
                                Label {
                                    anchors.verticalCenter: parent.verticalCenter
                                    width: appRow.width - 160
                                    text: appRow.modelData.name || ""
                                    size: 13
                                    font.weight: Font.Medium
                                    elide: Text.ElideRight
                                }
                            }
                            Row {
                                anchors.right: parent.right
                                anchors.rightMargin: 8
                                anchors.verticalCenter: parent.verticalCenter
                                spacing: 10
                                Item {
                                    visible: s.nyx
                                    width: 48
                                    height: 20
                                    Toggle {
                                        anchors.centerIn: parent
                                        checked: !!s.settings && !s.listed(s.settings.no_popups, appRow.modelData.app)
                                        onToggled: on => s.setListed("no_popups", appRow.modelData.app, !on)
                                    }
                                }
                                Item {
                                    width: 48
                                    height: 20
                                    Toggle {
                                        anchors.centerIn: parent
                                        checked: !appRow.modelData.excluded
                                        onToggled: on => {
                                            s.setListed("excluded", appRow.modelData.app, !on);
                                            s.client.request("Apps", {}, ok => s.apps = ok || s.apps);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
