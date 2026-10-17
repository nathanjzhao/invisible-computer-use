use crate::{Error, Result};
use crabstep::TypedStreamDeserializer;

pub const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
/// Decode inert typedstream data without instantiating Objective-C archive classes.
pub fn decode(text: Option<&str>, attributed: Option<&[u8]>) -> Result<Option<String>> {
    if let Some(text) = text.filter(|s| !s.is_empty()) {
        if text.len() > MAX_BODY_BYTES {
            return Err(Error::new(
                "messagesBodyTooLarge",
                "Message body exceeds its decode limit",
            ));
        }
        return Ok(Some(text.into()));
    }
    let Some(bytes) = attributed.filter(|b| !b.is_empty()) else {
        return Ok(text.map(str::to_string));
    };
    if bytes.len() > MAX_BODY_BYTES {
        return Err(Error::new(
            "messagesBodyTooLarge",
            "Attributed body exceeds its decode limit",
        ));
    }
    let mut archive = TypedStreamDeserializer::new(bytes);
    let root = archive.root().map_err(|_| {
        Error::new(
            "messagesBodyUnsupported",
            "Could not decode this attributed message body",
        )
    })?;
    root.as_string().map(|s| Some(s.into())).ok_or_else(|| {
        Error::new(
            "messagesBodyUnsupported",
            "Attributed message has no supported underlying string",
        )
    })
}
