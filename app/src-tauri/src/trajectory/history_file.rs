//! Native history segments are reviewed by immutable byte prefix, so capture can keep appending.
use super::*;
pub(super) struct Loaded {
    pub manifest: Manifest,
    pub events: Vec<(usize, Event)>,
    pub recording_sha256: String,
    pub manifest_sha256: String,
    pub reviewed_bytes: Option<u64>,
    pub format: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    id: String,
    timestamp_unix_ms: u64,
    kind: String,
    application: String,
    process_id: i64,
    window_id: i64,
    domain: Option<String>,
    payload: Value,
}
fn segment_bytes(path: &Path, limit: Option<u64>) -> Result<Vec<u8>, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| "history segment cannot be read")?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("select a regular history segment file".into());
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000);
    }
    let file = options
        .open(path)
        .map_err(|_| "history segment cannot be opened")?;
    let metadata = file
        .metadata()
        .map_err(|_| "history segment cannot be inspected")?;
    if !metadata.is_file() {
        return Err("select a regular history segment file".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("history segment reparse points are not supported".into());
        }
    }
    let length = limit.unwrap_or(metadata.len());
    if length == 0 || length > MAX_BYTES || metadata.len() < length {
        return Err("history segment changed or exceeds the 16 MiB review limit".into());
    }
    let mut bytes = Vec::new();
    file.take(length)
        .read_to_end(&mut bytes)
        .map_err(|_| "history segment read failed")?;
    if bytes.len() as u64 != length {
        return Err("history segment changed while reading".into());
    }
    // A completed native append ends with a newline. Never approve a torn last record.
    if bytes.last() != Some(&b'\n') {
        return Err(
            "history segment has an incomplete final record; retry after capture finishes writing"
                .into(),
        );
    }
    Ok(bytes)
}
pub(super) fn load_review(
    path: &Path,
    include_values: bool,
    limit: Option<u64>,
) -> Result<Loaded, String> {
    if path.is_dir() {
        if limit.is_some() {
            return Err("a history byte boundary cannot be applied to an observer folder".into());
        }
        let (manifest, events, hash, mut manifest_hash) = load(path)?;
        if include_values {
            manifest_hash = sha(format!("{manifest_hash}\0reviewed-values-v1").as_bytes());
        }
        return Ok(Loaded {
            manifest,
            events,
            recording_sha256: hash,
            manifest_sha256: manifest_hash,
            reviewed_bytes: None,
            format: "taiku-observer-v1".into(),
        });
    }
    let bytes = segment_bytes(path, limit)?;
    let contents = std::str::from_utf8(&bytes).map_err(|_| "history segment is not UTF-8")?;
    let mut events = Vec::new();
    let mut ids = std::collections::BTreeSet::new();
    for (index, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if events.len() >= MAX_EVENTS {
            return Err("too many history observations in one review".into());
        }
        let row: Row = serde_json::from_str(line)
            .map_err(|_| format!("invalid history observation at line {}", index + 1))?;
        if row.id.len() != 48
            || !row.id.bytes().all(|b| b.is_ascii_hexdigit())
            || !ids.insert(row.id.clone())
            || row.application.trim().is_empty()
            || row.application.len() > 240
            || row.application.chars().any(char::is_control)
            || row.process_id <= 0
            || row.window_id <= 0
            || !row.payload.is_object()
        {
            return Err(format!("invalid history identity at line {}", index + 1));
        }
        if row
            .domain
            .as_ref()
            .is_some_and(|d| !computer_history::policy::domain(d).is_ok_and(|v| v == *d))
        {
            return Err("history domain must be canonical and contain no full URL".into());
        }
        let timestamp = i64::try_from(row.timestamp_unix_ms)
            .ok()
            .and_then(|t| t.checked_mul(1_000_000))
            .ok_or("history timestamp overflow")?;
        let mut payload = row.payload;
        payload["history_event_id"] = serde_json::json!(row.id);
        payload["history_event_hash"] = serde_json::json!(sha(line.as_bytes()));
        let protected = payload
            .get("secure")
            .is_some_and(|v| v.as_bool() != Some(false))
            || payload
                .get("content_excluded")
                .is_some_and(|v| v.as_bool() != Some(false))
            || text(&payload, "role").contains("Secure")
            || text(&payload, "subrole").contains("Secure")
            || text(&payload, "value_kind") == "secure";
        payload["secure"] = serde_json::json!(protected);
        // Old native records did not have truncation flags. A field at its old cap is uncertain.
        if payload.get("truncated").is_none() {
            payload["capture_incomplete"] = serde_json::json!(
                text(&payload, "value").chars().count() >= 2048
                    || text(&payload, "label").chars().count() >= 512
                    || text(&payload, "identifier").chars().count() >= 256
            );
        }
        payload["app"] = serde_json::json!(row.application);
        payload["bundle_id"] = payload["app"].clone();
        payload["pid"] = serde_json::json!(row.process_id);
        payload["window_id"] = serde_json::json!(row.window_id);
        payload["domain"] = serde_json::json!(row.domain);
        // Older typing records have no explicit value kind. They are bursts of
        // input, never observations of the complete field they were typed into.
        if row.kind == "keyboard.text_input" {
            payload["value_kind"] = serde_json::json!("typing_fragment");
            payload["capture_incomplete"] = serde_json::json!(true);
        }
        let kind = match row.kind.as_str() {
            "app_focus" => "history.app_focus",
            "ui.focus" => "ui.focus",
            "ui.value" => "ui.value",
            "ui.snapshot" => "ui.snapshot",
            "ui.invoke" => "ui.invoke",
            "mouse.click" => "ui.click",
            "mouse.context_menu" => "ui.click",
            "mouse.drag" => "ui.drag",
            "keyboard.text_input" => "ui.value",
            "keyboard.shortcut" => "ui.shortcut",
            "keyboard.submit" => "ui.submit",
            "selection.changed" => "ui.selection",
            "terminal.value_changed" => "ui.terminal",
            "window.changed" => "ui.window",
            _ => "history.unsupported",
        };
        events.push((
            index + 1,
            Event {
                ts_mono_ns: timestamp,
                kind: kind.into(),
                payload,
            },
        ));
    }
    events.sort_by_key(|(line, e)| (e.ts_mono_ns, *line));
    let manifest_bytes=serde_json::to_vec(&serde_json::json!({"format":"computer-history-v1","clock":"UNIX_MS","include_values":include_values,"reviewed_bytes":bytes.len()})).unwrap();
    Ok(Loaded {
        manifest: Manifest {
            master_clock: "UNIX_MS".into(),
            anchors: vec![Anchor {
                mono_ns: 0,
                epoch_utc_ns: 0,
                qpf: 1,
            }],
        },
        events,
        recording_sha256: sha(&bytes),
        manifest_sha256: sha(&manifest_bytes),
        reviewed_bytes: Some(bytes.len() as u64),
        format: "computer-history-v1".into(),
    })
}
