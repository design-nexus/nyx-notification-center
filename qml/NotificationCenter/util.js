.pragma library

// Times as the GTK panel shows them (ui.rs): "now", "5 min", "14:15" today, "Yesterday",
// "Tue" this week, "Oct 28" before that.
function dayNumber(d) {
    return Math.floor((d.getTime() - d.getTimezoneOffset() * 60000) / 86400000);
}

function when(ts, now) {
    const ago = now - ts;
    if (ago < 60)
        return "now";
    if (ago < 3600)
        return Math.floor(ago / 60) + " min";
    const t = new Date(ts * 1000), n = new Date(now * 1000);
    const days = dayNumber(n) - dayNumber(t);
    if (days <= 0)
        return Qt.formatTime(t, "HH:mm");
    if (days === 1)
        return "Yesterday";
    if (days <= 6)
        return Qt.formatDate(t, "ddd");
    return Qt.formatDate(t, "MMM d");
}

// A day heading for history: "Today", "Yesterday", "Oct 28".
function day(ts, now) {
    const t = new Date(ts * 1000), n = new Date(now * 1000);
    const days = dayNumber(n) - dayNumber(t);
    if (days === 0)
        return "Today";
    if (days === 1)
        return "Yesterday";
    return Qt.formatDate(t, t.getFullYear() === n.getFullYear() ? "MMM d" : "MMM d, yyyy");
}

function sameDay(a, b) {
    return dayNumber(new Date(a * 1000)) === dayNumber(new Date(b * 1000));
}

function escape(s) {
    return String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

// Rich text marking each search word (case-insensitive), the rest escaped.
function marked(text, words, color) {
    if (!words || words.length === 0)
        return escape(text);
    const lower = String(text).toLowerCase();
    let spans = [];
    for (const w0 of words) {
        const w = w0.toLowerCase();
        if (!w)
            continue;
        let from = 0, i;
        while ((i = lower.indexOf(w, from)) >= 0) {
            spans.push([i, i + w.length]);
            from = i + w.length;
        }
    }
    spans.sort((a, b) => a[0] - b[0]);
    let out = "", at = 0;
    for (const [s, e] of spans) {
        if (s < at)
            continue;
        out += escape(text.substring(at, s)) + "<span style=\"background-color:" + color + "\">" + escape(text.substring(s, e)) + "</span>";
        at = e;
    }
    return out + escape(text.substring(at));
}

function flat(text) {
    return String(text || "").split(/\s+/).filter(x => x).join(" ");
}
