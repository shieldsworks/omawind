.pragma library

function label() {
    return "Wind";
}

function detail(s) {
    if (s.incompatible)
        return "omawind speaks another protocol version";
    if (!s.connected)
        return s.waited ? "omawind isn't running" : "Starting omawind…";
    if (s.summary)
        return s.strong ? s.summary + " — 21 kn or more: small-craft advisory strength" : s.summary;
    if (s.downloading)
        return "Downloading forecast…";
    return "No wind yet";
}
