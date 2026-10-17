// Transient pointer metadata for the host's existing History collector.
// The host owns consent, source policy, redaction and durable storage.
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Clutter from 'gi://Clutter';
import Meta from 'gi://Meta';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

const MAX_EVENTS = 64;
const LEASE_US = 2_000_000;

function bounds(value) {
    const row = {x: value.x, y: value.y, width: value.width, height: value.height};
    if (!Object.values(row).every(Number.isSafeInteger) || row.width <= 0 || row.height <= 0)
        throw new Error('History window geometry is unavailable');
    return row;
}
function contains(rect, x, y) {
    return x >= rect.x && y >= rect.y && x < rect.x + rect.width && y < rect.y + rect.height;
}
function nativeKeyFocus(win) {
    let actor = global.stage.get_key_focus();
    if (actor === null || actor === global.stage) return true;
    // Mutter can focus the client's surface actor. Bind its real window,
    // rather than treating every non-null actor as Shell UI or trusting a name.
    for (let depth = 0; actor && depth < 20; depth++) {
        if (typeof actor.get_meta_window === 'function') return actor.get_meta_window() === win;
        actor = typeof actor.get_parent === 'function' ? actor.get_parent() : null;
    }
    return false;
}

export class HistoryPointerSessions {
    constructor() {
        this.leases = new Map();
        this.timer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 250, () => {
            for (const [caller, lease] of this.leases)
                if (GLib.get_monotonic_time() > lease.until) this.stop(caller, lease.token);
            return GLib.SOURCE_CONTINUE;
        });
    }
    allowed(win) {
        return this.refusal(win) === null;
    }
    refusal(win) {
        if (!win) return 'window_missing';
        if (Main.sessionMode.isLocked !== false || Main.sessionMode.hasWindows !== true) return 'session_unavailable';
        if (Main.overview.visible !== false || Main.modalCount !== 0) return 'shell_modal';
        if (!nativeKeyFocus(win)) return 'shell_keyboard_focus';
        if (win !== global.display.focus_window || win.minimized) return 'window_inactive';
        if (win.get_client_type() !== Meta.WindowClientType.WAYLAND) return 'native_wayland_required';
        if (!global.get_window_actors().some(actor => actor.meta_window === win && actor.visible)) return 'window_not_visible';
        return null;
    }
    state(lease) {
        if (!this.allowed(lease.win) || lease.win.get_pid() !== lease.pid ||
            lease.win.get_stable_sequence() !== lease.id)
            throw new Error('History pointer window changed');
        return {id: lease.id, pid: lease.pid, frame: bounds(lease.win.get_frame_rect()),
            buffer: bounds(lease.win.get_buffer_rect()), monotonic_us: GLib.get_monotonic_time()};
    }
    start(caller, id, pid) {
        if (typeof caller !== 'string' || !caller.startsWith(':') || !Number.isSafeInteger(id) ||
            id <= 0 || !Number.isSafeInteger(pid) || pid <= 0)
            throw new Error('Invalid History pointer scope');
        const previous = this.leases.get(caller);
        if (previous) this.stop(caller, previous.token);
        if (this.leases.size >= 8) throw new Error('Too many History pointer clients');
        const win = global.get_window_actors().map(actor => actor.meta_window)
            .find(window => window?.get_stable_sequence() === id && window.get_pid() === pid);
        const refusal = this.refusal(win);
        if (refusal) {
            const focus = global.stage.get_key_focus();
            const detail = refusal === 'shell_keyboard_focus'
                ? ` (${focus === global.stage ? 'stage' : focus?.constructor?.name ?? 'unknown'})` : '';
            throw new Error(`History pointer window is unavailable: ${refusal}${detail}`);
        }
        const lease = {win, id, pid, token: GLib.uuid_string_random(), until: GLib.get_monotonic_time() + LEASE_US,
            events: [], sequence: 0, lost: false, changed: 0, removed: 0, watch: 0};
        const state = this.state(lease);
        this.leases.set(caller, lease);
        try {
            lease.changed = win.connect('notify::user-time', () => this.observe(caller, lease));
            lease.removed = win.connect('unmanaged', () => this.stop(caller, lease.token));
            lease.watch = Gio.bus_watch_name_on_connection(Gio.DBus.session, caller, Gio.BusNameWatcherFlags.NONE,
                null, () => this.stop(caller, lease.token));
            return {token: lease.token, sequence: lease.sequence, ...state};
        } catch (error) {
            this.stop(caller, lease.token); throw error;
        }
    }
    observe(caller, lease) {
        if (this.leases.get(caller) !== lease || GLib.get_monotonic_time() > lease.until) return;
        try {
            const state = this.state(lease);
            const event = Clutter.get_current_event();
            // A programmatic activation or Shell UI action is not client input.
            if (!event || event.get_source() !== null ||
                event.get_time() !== lease.win.get_user_time()) return;
            let input;
            if (event.type() === Clutter.EventType.BUTTON_PRESS) {
                const button = event.get_button();
                if (button !== 1 && button !== 3) return;
                const [x, y] = event.get_coords();
                if (!Number.isFinite(x) || !Number.isFinite(y) || !contains(state.buffer, x, y) ||
                    !contains(state.frame, x, y)) return;
                input = {button, x, y};
            } else if (event.type() === Clutter.EventType.KEY_PRESS) {
                // Never enqueue printable symbols, keycodes or text. This uses
                // the same live native-window lease as pointer observations.
                const symbol = event.get_key_symbol();
                const key = symbol === Clutter.KEY_Return || symbol === Clutter.KEY_KP_Enter ? 'return'
                    : symbol === Clutter.KEY_Tab || symbol === Clutter.KEY_ISO_Left_Tab ? 'tab'
                    : symbol === Clutter.KEY_Escape ? 'escape' : null;
                if (key === null) return;
                const modifiers = event.get_state();
                input = {button: 0, x: 0, y: 0, key,
                    ctrl: Boolean(modifiers & Clutter.ModifierType.CONTROL_MASK),
                    alt: Boolean(modifiers & Clutter.ModifierType.MOD1_MASK),
                    shift: Boolean(modifiers & Clutter.ModifierType.SHIFT_MASK),
                    win: Boolean(modifiers & Clutter.ModifierType.SUPER_MASK)};
            } else return;
            if (lease.events.length >= MAX_EVENTS) {
                lease.events = []; lease.lost = true; return;
            }
            if (!lease.lost) lease.events.push({sequence: ++lease.sequence, time: event.get_time(),
                timestamp_ms: Math.floor(GLib.get_real_time() / 1000), observed_monotonic_us: state.monotonic_us,
                ...input, frame: state.frame, buffer: state.buffer});
        } catch (_error) {
            lease.events = []; lease.lost = true;
        }
    }
    current(caller, token) {
        const lease = this.leases.get(caller);
        if (!lease || lease.token !== token) throw new Error('History pointer lease is unavailable');
        if (GLib.get_monotonic_time() > lease.until) {
            this.stop(caller, token); throw new Error('History pointer lease expired');
        }
        return lease;
    }
    read(caller, token) {
        const lease = this.current(caller, token);
        const state = this.state(lease);
        lease.until = GLib.get_monotonic_time() + LEASE_US;
        const result = {token, sequence: lease.sequence, ...state, events: lease.events, lost: lease.lost};
        lease.events = []; lease.lost = false;
        return result;
    }
    check(caller, token) {
        const lease = this.current(caller, token);
        return {token, ...this.state(lease)};
    }
    stop(caller, token) {
        const lease = this.leases.get(caller);
        if (!lease || lease.token !== token) return {stopped: false};
        this.leases.delete(caller);
        try { if (lease.changed) lease.win.disconnect(lease.changed); if (lease.removed) lease.win.disconnect(lease.removed); } catch (_error) {}
        if (lease.watch) Gio.bus_unwatch_name(lease.watch);
        lease.events = [];
        return {stopped: true};
    }
    close() {
        for (const [caller, lease] of this.leases) this.stop(caller, lease.token);
        if (this.timer) GLib.source_remove(this.timer);
        this.timer = 0;
    }
}
