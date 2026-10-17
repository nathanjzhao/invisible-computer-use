//! A verified page is transient context; only its domain belongs in the history envelope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub window_id: String,
    pub tab_id: String,
    pub bounds: [i32; 4],
    pub url: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Snapshot {
    Public(Page),
    Private,
    Other,
}
pub fn document_url(value: &str) -> Option<String> {
    if value.len() > 16384 {
        return None;
    }
    let u = url::Url::parse(value).ok()?;
    if !matches!(u.scheme(), "http" | "https") || u.host().is_none() {
        return None;
    }
    Some(u.to_string())
}
pub fn same_document(actual: &str, expected: &str) -> bool {
    document_url(actual).is_some_and(|a| document_url(expected).as_ref() == Some(&a))
}
pub fn chrome(application: &str) -> bool {
    ["com.google.Chrome", "org.chromium.Chromium"]
        .iter()
        .any(|a| application.eq_ignore_ascii_case(a))
}
