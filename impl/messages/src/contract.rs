//! Wire contract recovered from tools/list on a disposable Mac, 2026-10-09.
//! Data access and consent remain host responsibilities; parsing cannot approve a request.
use crate::{
    Error, Result,
    activity::{self, Breakdown},
    cursor::Cursors,
    database::{self, Filter},
    dates, invalid, worker,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::OnceLock};

pub fn tools() -> &'static Vec<Value> {
    static TOOLS: OnceLock<Vec<Value>> = OnceLock::new();
    TOOLS.get_or_init(|| {
        serde_json::from_str(include_str!("../schema/tools.json"))
            .expect("checked built-in schemas")
    })
}
fn schema(name: &str) -> Result<&'static Value> {
    tools()
        .iter()
        .find(|t| t["name"] == name)
        .ok_or_else(|| invalid("Unknown Messages tool"))
}

/// Validate the subset of JSON Schema used by the recovered tool contracts.
pub fn validate(value: &Value, schema: &Value) -> Result<()> {
    if let Some(kind) = schema["type"].as_str() {
        let matches = match kind {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "boolean" => value.is_boolean(),
            "integer" => value.as_f64().is_some_and(|n| n.fract() == 0.0),
            _ => return Err(invalid("Unsupported schema type")),
        };
        if !matches {
            return Err(invalid(format!("Expected {kind}")));
        }
    }
    if schema.get("const").is_some_and(|c| c != value) {
        return Err(invalid("Value does not match the required constant"));
    }
    if schema["enum"]
        .as_array()
        .is_some_and(|choices| !choices.contains(value))
    {
        return Err(invalid("Value is not one of the allowed choices"));
    }
    for (key, exact) in [("oneOf", true), ("anyOf", false)] {
        if let Some(variants) = schema[key].as_array() {
            let n = variants
                .iter()
                .filter(|s| validate(value, s).is_ok())
                .count();
            if n == 0 || (exact && n != 1) {
                return Err(invalid(format!("Arguments do not satisfy {key}")));
            }
        }
    }
    if let Some(object) = value.as_object() {
        if let Some(required) = schema["required"].as_array() {
            for field in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(field) {
                    return Err(invalid(format!("Missing required field: {field}")));
                }
            }
        }
        for (key, child) in object {
            if let Some(s) = schema["properties"].get(key) {
                validate(child, s).map_err(|e| invalid(format!("{key}: {}", e.message)))?;
            } else if schema["additionalProperties"] == false {
                return Err(invalid(format!("Unknown field: {key}")));
            }
        }
    }
    if let Some(items) = value.as_array() {
        bounds(items.len() as f64, schema, "minItems", "maxItems")?;
        if let Some(s) = schema.get("items") {
            for item in items {
                validate(item, s)?;
            }
        }
    }
    if let Some(text) = value.as_str() {
        bounds(
            text.chars().count() as f64,
            schema,
            "minLength",
            "maxLength",
        )?;
        if schema["format"] == "date-time" {
            dates::parse(text)?;
        }
    }
    if let Some(n) = value.as_f64() {
        bounds(n, schema, "minimum", "maximum")?;
    }
    Ok(())
}
fn bounds(n: f64, s: &Value, min: &str, max: &str) -> Result<()> {
    if s[min].as_f64().is_some_and(|v| n < v) || s[max].as_f64().is_some_and(|v| n > v) {
        return Err(invalid("Value is outside the permitted range"));
    }
    Ok(())
}

pub trait Contacts {
    fn begin_request(&self, _cancel: std::sync::Arc<std::sync::atomic::AtomicBool>) {}
    fn resolve(&self, participant: &str) -> Result<Vec<String>>;
    fn candidates(&self, participant: &str) -> Result<Vec<crate::contacts::Person>> {
        if let Some(person) = crate::contacts::Person::direct(participant) {
            return Ok(vec![person]);
        }
        Ok(vec![crate::contacts::Person {
            identifier: format!("resolved:{participant}"),
            name: participant.into(),
            handles: self.resolve(participant)?,
        }])
    }
    fn name(&self, _handle: &str) -> Option<String> {
        None
    }
}
/// Direct-address fallback; it never treats an unresolved person's name as an address.
pub struct AddressesOnly;
impl Contacts for AddressesOnly {
    fn resolve(&self, participant: &str) -> Result<Vec<String>> {
        let p = participant.trim();
        if crate::contacts::is_address(p) {
            Ok(vec![p.into()])
        } else {
            Err(Error::new(
                "messagesContactResolutionUnavailable",
                "Resolve this person's name with macOS Contacts or provide a phone number or email address",
            ))
        }
    }
}

pub enum Invocation {
    Query {
        query: Box<worker::Query>,
        cursor: Option<String>,
        key: [u8; 32],
    },
    Image {
        id: String,
    },
    Send {
        arguments: Value,
    },
}
pub fn parse(
    name: &str,
    arguments: Value,
    time_zone: &str,
    contacts: &dyn Contacts,
) -> Result<Invocation> {
    validate(&arguments, &schema(name)?["inputSchema"])?;
    let v = &arguments;
    if name == "read_image" {
        return Ok(Invocation::Image {
            id: v["id"].as_str().unwrap().into(),
        });
    }
    if name == "send_message" {
        return Ok(Invocation::Send { arguments });
    }
    let mut filter = Filter {
        from: v["from"].as_str().map(dates::parse).transpose()?,
        to: v["to"].as_str().map(dates::parse).transpose()?,
        unread_only: v["unread_only"].as_bool().unwrap_or(false),
        exact_participants: v["exact_participants"].as_bool().unwrap_or(false),
        text: v["text"].as_str().map(str::to_owned),
        ..Default::default()
    };
    if let Some(participants) = v["participants"].as_array() {
        for p in participants {
            filter
                .participants
                .push(contacts.resolve(p.as_str().unwrap())?);
        }
    }
    if let Some(chats) = v["chat_guids"].as_array() {
        filter.chat_guids = chats
            .iter()
            .map(|g| g.as_str().unwrap().to_owned())
            .collect();
    }
    if let Some(chat) = v["chat_guid"].as_str() {
        filter.chat_guids = vec![chat.into()];
    }
    if let Some(kind) = v.get("chat_type") {
        filter.chat_type =
            serde_json::from_value(kind.clone()).map_err(|_| invalid("Invalid chat type"))?;
    }
    filter = filter.normalize()?;
    let cursor = v["cursor"].as_str().map(str::to_owned);
    let limit = v["limit"]
        .as_f64()
        .map(|n| n as usize)
        .unwrap_or(if name == "find_chats" { 20 } else { 50 });
    let (query, key) = match name {
        "find_chats" => {
            let name = v["name"].as_str().map(str::to_owned);
            let key = Cursors::query_key(&("find_chats", &filter, &name))?;
            (
                worker::Query::Find {
                    filter,
                    name,
                    limit,
                },
                key,
            )
        }
        "read_messages" | "search_messages" => {
            let key = Cursors::query_key(&(name, &filter))?;
            (
                worker::Query::Read {
                    filter,
                    page: None,
                    limit,
                },
                key,
            )
        }
        "count_message_activity" => {
            let query = activity::Query {
                filter,
                interval: serde_json::from_value(
                    v.get("interval").cloned().unwrap_or(json!("total")),
                )
                .unwrap(),
                time_zone: time_zone.into(),
                breakdown: serde_json::from_value(
                    v.get("breakdown").cloned().unwrap_or(json!("overall")),
                )
                .unwrap(),
                rank_by: serde_json::from_value(
                    v.get("rank_by").cloned().unwrap_or(json!("total")),
                )
                .unwrap(),
            };
            let key = query.key()?;
            let limit = v["chat_limit"].as_f64().map(|n| n as usize).unwrap_or(20);
            (
                worker::Query::Count {
                    query,
                    page: None,
                    limit,
                },
                key,
            )
        }
        _ => unreachable!(),
    };
    Ok(Invocation::Query {
        query: Box::new(query),
        cursor,
        key,
    })
}

#[derive(Default)]
struct People {
    rows: Vec<Value>,
    ids: BTreeMap<String, String>,
}
impl People {
    fn id(&mut self, handle: &str, contacts: &dyn Contacts, prefix: &str) -> String {
        if let Some(id) = self.ids.get(handle) {
            return id.clone();
        }
        let id = format!("{prefix}{}", self.rows.len() + 1);
        let mut row = json!({"local_ref":id,"handle":handle});
        if let Some(name) = contacts.name(handle) {
            row["name"] = json!(name);
        }
        self.rows.push(row);
        self.ids.insert(handle.into(), id.clone());
        id
    }
    fn members(&mut self, chat: &database::Chat, contacts: &dyn Contacts) -> Vec<String> {
        chat.participants
            .iter()
            .map(|h| self.id(h, contacts, "p"))
            .collect()
    }
}
fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| {
        Error::new(
            "messagesWorkerFailed",
            "Query worker returned an invalid data shape",
        )
    })
}

pub fn render(
    name: &str,
    data: Value,
    contacts: &dyn Contacts,
    breakdown: Breakdown,
) -> Result<Value> {
    let result = match name {
        "find_chats" => {
            let page: database::ChatPage = decode(data)?;
            let mut people = People::default();
            let chats = page.chats.iter().map(|c|json!({"chat_guid":c.chat_guid,"display_name":c.name.as_deref().unwrap_or(""),"service":c.service.as_deref().unwrap_or(""),"last_message_date":c.last_message_date,"participants":people.members(c,contacts),"unread_count":c.unread_count})).collect::<Vec<_>>();
            json!({"chats":chats,"participants":people.rows,"has_more":page.has_more})
        }
        "read_messages" | "search_messages" => {
            let page: database::MessagePage = decode(data)?;
            let mut senders = People::default();
            let mut chats = Vec::<Value>::new();
            let mut chat_ids = BTreeMap::new();
            let mut messages = Vec::new();
            for message in page.messages {
                if message.body_error.is_some() {
                    return Err(Error::new(
                        "messagesBodyUnsupported",
                        "One message body could not be decoded; no partial text result was returned",
                    ));
                }
                let next = format!("c{}", chats.len() + 1);
                let chat_ref = chat_ids
                    .entry(message.chat_guid.clone())
                    .or_insert_with(|| {
                        chats.push(json!({"local_ref":next,"chat_guid":message.chat_guid}));
                        next
                    });
                let sender = if message.from_me {
                    "me".into()
                } else if let Some(handle) = message.sender {
                    senders.id(&handle, contacts, "s")
                } else {
                    "unknown".into()
                };
                let mut row = json!({"chat":chat_ref,"sender":sender,"date":message.date});
                if let Some(text) = message.text {
                    row["body"] = json!(text);
                }
                if message.is_unread {
                    row["is_unread"] = json!(true);
                }
                if !message.attachments.is_empty() {
                    row["attachments"] = json!(
                        message
                            .attachments
                            .into_iter()
                            .map(|a| {
                                let mut entry = json!({"id":a.id});
                                if let Some(filename) = a.filename {
                                    entry["filename"] = json!(filename);
                                }
                                if let Some(mime) = a.mime_type {
                                    entry["mime_type"] = json!(mime);
                                }
                                if let Some(size) = a.size.filter(|&n| n >= 0) {
                                    entry["size_bytes"] = json!(size);
                                }
                                entry
                            })
                            .collect::<Vec<_>>()
                    );
                }
                messages.push(row);
            }
            let mut response = json!({"chats":chats,"senders":senders.rows,"messages":messages});
            if let Some(cursor) = page.next_cursor {
                response["next_cursor"] = json!(cursor);
            }
            response
        }
        "count_message_activity" => {
            let report: activity::Report = decode(data)?;
            let mut people = People::default();
            let mut result = json!({"breakdown":breakdown,"range":{"from":report.from,"to":report.to},"interval":report.interval,"time_zone":report.time_zone,"buckets":report.buckets,"matching_chat_count":report.matching_chat_count});
            if breakdown == Breakdown::Overall {
                result["overall_activity"] = json!(report.overall_activity);
            } else {
                result["chats"] = json!(report.chats.iter().map(|row| {
                    let c = &row.chat; let mut value = json!(row.activity);
                    let obj = value.as_object_mut().unwrap();
                    obj.extend(json!({"chat_guid":c.chat_guid,"display_name":c.name.as_deref().unwrap_or(""),"service":c.service.as_deref().unwrap_or(""),"type":if c.is_group {"group"}else{"direct"},"participants":people.members(c,contacts)}).as_object().unwrap().clone());value
                }).collect::<Vec<_>>());
                result["participants"] = json!(people.rows);
                if let Some(cursor) = report.next_cursor {
                    result["next_cursor"] = json!(cursor);
                }
            }
            result
        }
        _ => return Err(invalid("This tool does not return a database page")),
    };
    validate(&result, &schema(name)?["outputSchema"]).map_err(|_| {
        Error::new(
            "messagesContractError",
            "Query response failed its output contract",
        )
    })?;
    Ok(result)
}
