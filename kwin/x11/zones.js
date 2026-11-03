// Loaded only in KWin X11. The Rust host supplies the per-instance epoch.
// Observe native moves; never replace the user's tile tree or global bindings.
let drag = null;
let nextId = 0;
let busy = false;
let timer = new QTimer();
timer.interval = 32;

function request(kind, current) {
    if (!current || (busy && kind === "update")) return;
    const window = current.window;
    const sequence = ++current.sequence;
    const pointer = workspace.cursorPos;
    const sent = Date.now();
    const value = {epoch: epoch, id: current.id, sequence: sequence, kind: kind,
        position: [pointer.x, pointer.y], app: String(window.resourceClass),
        desktopFile: String(window.desktopFileName)};
    busy = true;
    callDBus("org.waytoys.Runtime1", "/Waytoys", "org.waytoys.Runtime1", "X11Event", JSON.stringify(value), function (raw) {
        busy = false;
        if (kind !== "drop" || !raw || Date.now() - sent > 150 || nextId !== current.id) return;
        const answer = JSON.parse(raw);
        if (answer.epoch !== epoch || answer.id !== current.id || answer.sequence !== sequence) return;
        if (window.move || window.resize || window.deleted || window.minimized || window.fullScreen) return;
        if (answer.cancelled) {
            window.tile = current.tile;
            const r = current.start;
            window.frameGeometry = {x:r[0],y:r[1],width:r[2],height:r[3]};
            return;
        }
        if (!answer.rect) return;
        const r = answer.rect;
        const frame = window.frameGeometry;
        const client = window.clientGeometry;
        const min = window.minSize;
        if (min && client && (min.width + frame.width - client.width > r[2] + 1 || min.height + frame.height - client.height > r[3] + 1)) return;
        window.setMaximize(false, false);
        window.tile = null;
        window.frameGeometry = {x:r[0], y:r[1], width:r[2], height:r[3]};
    });
}
timer.timeout.connect(function () { if (drag) request("update", drag); });
function track(window) {
    window.interactiveMoveResizeStarted.connect(function () {
        if (!window.move || !window.normalWindow || !window.moveable || !window.resizeable || window.fullScreen || String(window.resourceClass).indexOf("waytoys") === 0) return;
        const r = window.frameGeometry;
        drag = {window:window, id:++nextId, sequence:0, start:[r.x,r.y,r.width,r.height], tile:window.tile};
        busy = false;
        request("update", drag);
        timer.start();
    });
    window.interactiveMoveResizeFinished.connect(function () {
        if (!drag || drag.window !== window) return;
        timer.stop();
        const current = drag;
        drag = null;
        const r = window.frameGeometry;
        const a = current.start;
        // KWin restores the initial frame on Esc. A drop back at the exact
        // starting frame also leaves it alone, avoiding a snap after cancel.
        const cancelled = r.x === a[0] && r.y === a[1] && r.width === a[2] && r.height === a[3];
        request(cancelled ? "cancel" : "drop", current);
    });
    window.closed.connect(function () {
        if (drag && drag.window === window) {request("cancel",drag);drag=null;timer.stop();}
    });
}
workspace.windowList().forEach(track);
workspace.windowAdded.connect(track);
