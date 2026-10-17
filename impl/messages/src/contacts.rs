//! Contacts resolution keeps people separate from their alternative addresses.
use crate::{Error, Result, contract::Contacts, database::normalize_handle};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, HashMap},
    path::PathBuf,
    sync::{Arc, Mutex, atomic::AtomicBool},
    time::Duration,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Person {
    pub identifier: String,
    pub name: String,
    pub handles: Vec<String>,
}
impl Person {
    pub fn direct(value: &str) -> Option<Self> {
        is_address(value).then(|| Self {
            identifier: format!("address:{}", normalize_handle(value)),
            name: value.trim().into(),
            handles: vec![value.trim().into()],
        })
    }
}
pub fn is_address(value: &str) -> bool {
    let value = value.trim();
    if let Some((left, right)) = value.split_once('@') {
        return !left.is_empty()
            && !right.is_empty()
            && !right.contains('@')
            && !value.chars().any(|c| c.is_whitespace() || c.is_control());
    }
    value.chars().any(|c| c.is_ascii_digit())
        && value
            .chars()
            .all(|c| c.is_ascii_digit() || "+()- .".contains(c))
}
pub struct Native {
    executable: PathBuf,
    cancel: Mutex<Arc<AtomicBool>>,
    names: Mutex<HashMap<String, Option<String>>>,
}
impl Native {
    pub fn new(executable: PathBuf) -> Result<Self> {
        if !executable.is_absolute() {
            return Err(crate::invalid("Contacts helper path must be absolute"));
        }
        Ok(Self {
            executable,
            cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
            names: Mutex::new(HashMap::new()),
        })
    }
    fn request(&self, operation: &str, query: Option<&str>) -> Result<Value> {
        let cancel = self.cancel.lock().map_err(|_| unavailable())?.clone();
        invoke(
            &self.executable,
            json!({"operation":operation,"query":query}),
            cancel,
            Duration::from_secs(8),
        )
    }
    fn people(&self, operation: &str, query: &str) -> Result<Vec<Person>> {
        let value = self.request(operation, Some(query))?;
        let people: Vec<Person> =
            serde_json::from_value(value["people"].clone()).map_err(|_| unavailable())?;
        if people.len() > 200
            || people.iter().any(|p| {
                p.identifier.is_empty()
                    || p.identifier.len() > 1000
                    || p.name.len() > 4096
                    || p.handles.len() > 100
                    || p.handles.iter().any(|s| s.len() > 1000)
            })
        {
            return Err(unavailable());
        }
        Ok(people)
    }
}
pub use crate::native_bridge::invoke;
impl Contacts for Native {
    fn begin_request(&self, cancel: Arc<AtomicBool>) {
        if let Ok(mut active) = self.cancel.lock() {
            *active = cancel;
        }
        if let Ok(mut names) = self.names.lock() {
            names.clear();
        }
    }
    fn resolve(&self, query: &str) -> Result<Vec<String>> {
        if let Some(person) = Person::direct(query) {
            return Ok(person.handles);
        }
        let people = self.people("name", query)?;
        let mut seen = BTreeSet::new();
        let mut handles = Vec::new();
        for person in people {
            for handle in person.handles {
                if is_address(&handle) && seen.insert(normalize_handle(&handle)) {
                    handles.push(handle);
                }
            }
        }
        if handles.is_empty() {
            return Err(Error::new(
                "messagesContactNotFound",
                "No accessible contact with a phone number or email address matched this name",
            ));
        }
        Ok(handles)
    }
    fn candidates(&self, query: &str) -> Result<Vec<Person>> {
        if let Some(person) = Person::direct(query) {
            return Ok(vec![person]);
        }
        let people = self
            .people("name", query)?
            .into_iter()
            .filter(|p| p.handles.iter().any(|h| is_address(h)))
            .collect::<Vec<_>>();
        if people.is_empty() {
            return Err(Error::new(
                "messagesContactNotFound",
                "No accessible contact matched this recipient",
            ));
        }
        Ok(people)
    }
    fn name(&self, handle: &str) -> Option<String> {
        let key = normalize_handle(handle);
        if let Ok(cache) = self.names.lock()
            && let Some(name) = cache.get(&key)
        {
            return name.clone();
        }
        let name = self
            .people("address", handle)
            .ok()
            .filter(|p| p.len() == 1)
            .and_then(|p| p.first().map(|p| p.name.clone()))
            .filter(|s| !s.is_empty());
        if let Ok(mut cache) = self.names.lock()
            && cache.len() < 1000
        {
            cache.insert(key, name.clone());
        }
        name
    }
}
fn unavailable() -> Error {
    Error::new(
        "messagesContactsUnavailable",
        "The native Contacts helper is unavailable or returned invalid data",
    )
}
