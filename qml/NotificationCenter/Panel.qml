import QtQuick
import Nyx.Ui
import "util.js" as U

// The panel's content: the header (views, Do not disturb, settings, Clear All) over the
// Inbox, Pinned or History, or the settings in their place.
FocusScope {
    id: p

    required property var client
    property var lists: ({ inbox: [], pinned: [], error: null })
    property bool dnd: false
    property string mode: "omarchy"
    property string view: "inbox"
    property bool settingsOpen: false
    property real maxListHeight: 600
    property bool shown: false
    property real now: Date.now() / 1000
    property var expanded: ({})

    signal hide
    signal setSettingsOpen(bool on)

    implicitHeight: settingsOpen ? settingsView.implicitHeight : main.implicitHeight

    function setSearch(text) {
        search.text = text;
    }
    function reset() {
        expanded = {};
        search.clear();
        appIndex = 0;
        daysIndex = 0;
        now = Date.now() / 1000;
    }

    Timer {
        interval: 30000
        repeat: true
        running: p.shown
        onTriggered: p.now = Date.now() / 1000
    }

    Keys.onEscapePressed: event => {
        if (settingsOpen)
            setSettingsOpen(false);
        else
            hide();
        event.accepted = true;
    }

    // ---------- Inbox ----------
    function inboxRows() {
        if (lists.error)
            return [{ kind: "empty", icon: "dialog-warning-symbolic", text: lists.error }];
        const inbox = lists.inbox || [], pinned = lists.pinned || [];
        let r = [];
        if (pinned.length > 0) {
            r.push({ kind: "pinnedHead" });
            for (const n of pinned.slice(0, 2))
                r.push({ kind: "card", note: n, place: "pinned" });
        }
        if (inbox.length === 0) {
            r.push({ kind: "empty", icon: "preferences-system-notifications-symbolic", text: "No new notifications" });
            return r;
        }
        if (pinned.length > 0)
            r.push({ kind: "section", text: "Notifications" });
        // Groups by app, the most recent group first.
        let order = [];
        for (const n of inbox)
            if (order.indexOf(n.app) < 0)
                order.push(n.app);
        for (const app of order) {
            const group = inbox.filter(n => n.app === app);
            if (group.length === 1)
                r.push({ kind: "card", note: group[0], place: "inbox" });
            else if (expanded[app]) {
                r.push({ kind: "groupHead", app: app, note: group[0], ids: group.map(n => n.id) });
                for (const n of group)
                    r.push({ kind: "card", note: n, place: "inbox" });
            } else
                r.push({ kind: "stack", app: app, note: group[0], count: group.length });
        }
        return r;
    }
    function pinnedRows() {
        if (lists.error)
            return [{ kind: "empty", icon: "dialog-warning-symbolic", text: lists.error }];
        const pinned = lists.pinned || [];
        if (pinned.length === 0)
            return [{ kind: "empty", icon: "view-pin-symbolic", text: "Pin a notification to keep it here" }];
        return pinned.map(n => ({ kind: "card", note: n, place: "pinned" }));
    }

    // ---------- History ----------
    property var history: []
    property string historyError: ""
    property var filterApps: []
    property int appIndex: 0
    property int daysIndex: 0
    readonly property var words: search.text.split(/\s+/).filter(w => w)
    readonly property bool historyShown: shown && !settingsOpen && view === "history"

    function readHistory() {
        if (!historyShown)
            return;
        client.request("Apps", {}, ok => {
            if (!ok)
                return;
            const names = ok.filter(a => a.count > 0);
            if (JSON.stringify(names.map(a => a.app)) !== JSON.stringify(filterApps.map(a => a.app))) {
                const chosen = appIndex > 0 && filterApps[appIndex - 1] ? filterApps[appIndex - 1].app : "";
                filterApps = names;
                const at = appIndex > 0 ? names.findIndex(a => a.app === chosen) : -1;
                appIndex = at >= 0 ? at + 1 : 0;
            }
        });
        const q = { text: search.text, app: appIndex > 0 && filterApps[appIndex - 1] ? filterApps[appIndex - 1].app : "", days: [0, 1, 7, 30][daysIndex], limit: 300 };
        client.request("History", q, (ok, err) => {
            historyError = err ? err.msg : "";
            if (ok)
                history = ok;
        });
    }
    Timer {
        id: historySoon
        interval: 150
        onTriggered: p.readHistory()
    }
    onHistoryShownChanged: if (historyShown)
        readHistory()
    onAppIndexChanged: historySoon.restart()
    onDaysIndexChanged: historySoon.restart()
    Connections {
        target: p.client
        function onEvent(ev, body) {
            if (ev === "Changed")
                historySoon.restart();
        }
    }
    function historyRows() {
        if (historyError)
            return [{ kind: "empty", icon: "dialog-warning-symbolic", text: historyError }];
        if (history.length === 0) {
            const plain = search.text === "" && appIndex === 0 && daysIndex === 0;
            return [{ kind: "empty", icon: "edit-find-symbolic", text: plain ? "No history yet" : "Nothing matches" }];
        }
        let r = [], last = null;
        for (const n of history) {
            if (last === null || !U.sameDay(last, n.ts)) {
                r.push({ kind: "section", text: U.day(n.ts, now) });
                last = n.ts;
            }
            r.push({ kind: "card", note: n, place: "history" });
        }
        return r;
    }

    Column {
        id: main
        visible: !p.settingsOpen
        width: parent.width
        spacing: 10

        Item {
            width: parent.width
            height: 32
            Segmented {
                id: tabs
                anchors.verticalCenter: parent.verticalCenter
                width: 3 * 56 + 8
                options: [
                    { id: "inbox", icon: "preferences-system-notifications-symbolic" },
                    { id: "pinned", icon: "view-pin-symbolic" },
                    { id: "history", icon: "document-open-recent-symbolic" }
                ]
                current: p.view
                onChosen: id => p.view = id
            }
            Row {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                spacing: 4
                IconButton {
                    icon: p.dnd ? "notifications-disabled-symbolic" : "weather-clear-night-symbolic"
                    toggled: p.dnd
                    tooltip: p.dnd ? "Do not disturb is on" : "Do not disturb"
                    onClicked: p.client.request("ToggleDnd", {}, null)
                }
                IconButton {
                    icon: "emblem-system-symbolic"
                    tooltip: "Notification settings"
                    onClicked: p.setSettingsOpen(true)
                }
                IconButton {
                    visible: p.view === "inbox"
                    icon: "edit-clear-all-symbolic"
                    tooltip: "Clear all (pinned ones stay)"
                    onClicked: p.client.request("DismissAll", {}, null)
                }
            }
        }

        NoteList {
            visible: p.view === "inbox"
            width: parent.width
            client: p.client
            rows: p.view === "inbox" ? p.inboxRows() : []
            now: p.now
            maxHeight: p.maxListHeight
            onDone: p.hide()
            onExpand: app => {
                const e = Object.assign({}, p.expanded);
                e[app] = true;
                p.expanded = e;
            }
            onFold: app => {
                const e = Object.assign({}, p.expanded);
                delete e[app];
                p.expanded = e;
            }
            onShowPinned: p.view = "pinned"
        }
        NoteList {
            visible: p.view === "pinned"
            width: parent.width
            client: p.client
            rows: p.view === "pinned" ? p.pinnedRows() : []
            place: "pinned"
            now: p.now
            maxHeight: p.maxListHeight
            onDone: p.hide()
        }
        Column {
            visible: p.view === "history"
            width: parent.width
            spacing: 8
            SearchField {
                id: search
                width: parent.width
                placeholder: "Search notifications"
                onTextChanged: historySoon.restart()
                onEscaped: p.hide()
            }
            Row {
                spacing: 6
                Choice {
                    items: ["All apps"].concat(p.filterApps.map(a => a.name))
                    index: p.appIndex
                    menuWidth: 220
                    onChosen: i => p.appIndex = i
                }
                Choice {
                    items: ["Any time", "Today", "Last 7 days", "Last 30 days"]
                    index: p.daysIndex
                    onChosen: i => p.daysIndex = i
                }
            }
            NoteList {
                width: parent.width
                client: p.client
                rows: p.view === "history" ? p.historyRows() : []
                place: "history"
                words: p.words
                now: p.now
                maxHeight: p.maxListHeight - 76
                onDone: p.hide()
            }
        }
    }

    SettingsView {
        id: settingsView
        visible: p.settingsOpen
        width: parent.width
        client: p.client
        dnd: p.dnd
        nyx: p.mode === "nyx"
        shown: p.shown && p.settingsOpen
        maxHeight: p.maxListHeight
        onBack: p.setSettingsOpen(false)
    }
}
