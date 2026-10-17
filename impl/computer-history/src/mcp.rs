//! Exact five-tool control surface. The trusted collector is a separate writer, not a model tool.
use crate::{policy::Settings, store::Store, Error, Result};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
pub fn tools() -> Value {
    serde_json::from_str(include_str!("../tools.json")).expect("checked tool definitions")
}
pub struct Session {
    store: Store,
    read_revision: Option<(String, u64)>,
}
impl Session {
    pub fn new(store: Store) -> Self {
        Self {
            store,
            read_revision: None,
        }
    }
    pub fn call(&mut self, name: &str, args: Value) -> Result<Value> {
        if name != "computer_history_update_settings" && args != json!({}) {
            return Err(Error::invalid("This tool takes an empty arguments object"));
        }
        match name {
            "computer_history_get_settings" => {
                let (settings, generation, revision) = self.store.settings()?;
                self.read_revision = Some((generation, revision));
                serde_json::to_value(settings).map_err(|_| Error::invalid("Invalid settings"))
            }
            "computer_history_status" => self.store.status(),
            "computer_history_pause" => {
                self.store.pause(true)?;
                self.store.status()
            }
            "computer_history_resume" => {
                self.store.pause(false)?;
                self.store.status()
            }
            "computer_history_update_settings" => {
                let settings:Settings=serde_json::from_value(args).map_err(|_|Error::invalid("Provide the complete observation settings object with valid app and URL rules"))?;
                let (generation, revision) = self.read_revision.take().ok_or_else(|| {
                    Error::new(
                        "computerHistorySettingsNotRead",
                        "Call computer_history_get_settings immediately before replacing settings",
                    )
                })?;
                let state = self.store.update(settings, Some((&generation, revision)))?;
                serde_json::to_value(state.settings).map_err(|_| Error::invalid("Invalid settings"))
            }
            _ => Err(Error::new("unknownTool", "Unknown Computer History tool")),
        }
    }
}
fn frame(reader: &mut impl BufRead) -> std::io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    loop {
        let part = reader.fill_buf()?;
        if part.is_empty() {
            return if bytes.is_empty() {
                Ok(None)
            } else {
                Err(std::io::Error::other("Truncated request"))
            };
        }
        let n = part
            .iter()
            .position(|b| *b == b'\n')
            .map(|n| n + 1)
            .unwrap_or(part.len());
        if bytes.len() + n > 1024 * 1024 {
            return Err(std::io::Error::other("Request exceeds 1 MiB"));
        }
        let end = part[n - 1] == b'\n';
        bytes.extend_from_slice(&part[..n]);
        reader.consume(n);
        if end {
            return Ok(Some(bytes));
        }
    }
}
pub fn serve(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    store: Store,
) -> std::io::Result<()> {
    let mut session = Session::new(store);
    let mut initialized = false;
    let mut ready = false;
    while let Some(line) = frame(reader)? {
        let value: Value = match serde_json::from_slice(&line) {
            Ok(v) => v,
            Err(_) => {
                writeln!(
                    writer,
                    "{}",
                    json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Invalid JSON"}})
                )?;
                writer.flush()?;
                continue;
            }
        };
        let Some(id) = value.get("id").cloned() else {
            if value["jsonrpc"] == "2.0"
                && value["method"] == "notifications/initialized"
                && initialized
            {
                ready = true;
            }
            continue;
        };
        let error = |code, message: &str| json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}});
        let result = if value["jsonrpc"] != "2.0" || !(id.is_string() || id.is_i64() || id.is_u64())
        {
            error(-32600, "Invalid request")
        } else {
            match value["method"].as_str() {
                Some("initialize") if !initialized => {
                    if !value["params"]["protocolVersion"].is_string()
                        || !value["params"]["capabilities"].is_object()
                        || !value["params"]["clientInfo"]["name"].is_string()
                    {
                        writeln!(
                            writer,
                            "{}",
                            error(-32602, "Invalid initialization parameters")
                        )?;
                        writer.flush()?;
                        continue;
                    }
                    initialized = true;
                    let version = if value["params"]["protocolVersion"] == "2024-11-05" {
                        "2024-11-05"
                    } else {
                        "2025-06-18"
                    };
                    json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":version,"serverInfo":{"name":"Copilot Computer History","version":env!("CARGO_PKG_VERSION")},"capabilities":{"tools":{"listChanged":false}}}})
                }
                Some("ping") => json!({"jsonrpc":"2.0","id":id,"result":{}}),
                _ if !ready => error(-32002, "Initialize the MCP session first"),
                Some("tools/list") => json!({"jsonrpc":"2.0","id":id,"result":{"tools":tools()}}),
                Some("tools/call") => {
                    let name = value["params"]["name"].as_str().unwrap_or("");
                    let args = value["params"]
                        .get("arguments")
                        .cloned()
                        .unwrap_or(json!({}));
                    match session.call(name, args) {
                        Ok(data) => {
                            json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":data.to_string()}],"structuredContent":data,"isError":false}})
                        }
                        Err(e) if e.code == "unknownTool" => error(-32602, &e.message),
                        Err(e) => {
                            json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":e.message}],"structuredContent":{"message":e.message},"isError":true,"_meta":{"icu.error_code":e.code}}})
                        }
                    }
                }
                _ => error(-32601, "Unknown method"),
            }
        };
        writeln!(writer, "{result}")?;
        writer.flush()?;
    }
    Ok(())
}
