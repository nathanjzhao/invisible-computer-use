use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use url::{Host, Url};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Behavior {
    Observe,
    DoNotObserve,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "scope", rename_all = "lowercase", deny_unknown_fields)]
pub enum Rule {
    App {
        #[serde(rename = "bundleID")]
        bundle_id: String,
    },
    Url {
        #[serde(rename = "urlDomain")]
        url_domain: String,
    },
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Observation {
    pub default_application_behavior: Behavior,
    #[serde(rename = "defaultURLBehavior")]
    pub default_url_behavior: Behavior,
    pub allowlist: Vec<Rule>,
    pub blocklist: Vec<Rule>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub observation: Observation,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            observation: Observation {
                default_application_behavior: Behavior::DoNotObserve,
                default_url_behavior: Behavior::DoNotObserve,
                allowlist: vec![],
                blocklist: vec![],
            },
        }
    }
}
/// Metadata from the trusted collector, before it reads any optional content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Context {
    pub application: String,
    pub process_id: i64,
    pub window_id: i64,
    pub website: Website,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Website {
    NotWeb,
    Unknown,
    Url(String),
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Allowed,
    ApplicationBlocked,
    WebsiteBlocked,
    UnknownApplication,
    UnknownWebsite,
}
impl Settings {
    pub fn validated(mut self) -> Result<Self> {
        let o = &mut self.observation;
        for rules in [&mut o.allowlist, &mut o.blocklist] {
            if rules.len() > 1000 {
                return Err(Error::invalid(
                    "At most 1000 observation rules are supported",
                ));
            }
            let mut seen = BTreeSet::new();
            for rule in rules.iter_mut() {
                match rule {
                    Rule::App { bundle_id } => {
                        if bundle_id.is_empty()
                            || bundle_id.len() > 512
                            || bundle_id.trim() != bundle_id
                            || bundle_id.chars().any(|c| c.is_control())
                        {
                            return Err(Error::invalid(
                                "App rules require a nonempty application identifier",
                            ));
                        }
                    }
                    Rule::Url { url_domain } => *url_domain = domain(url_domain)?,
                }
            }
            rules.retain(|rule| seen.insert(serde_json::to_string(rule).unwrap()));
        }
        Ok(self)
    }
    pub fn evaluate(&self, context: &Context) -> Decision {
        let application = self.evaluate_application(context);
        if application != Decision::Allowed {
            return application;
        }
        let o = &self.observation;
        self.evaluate_website(&context.website, o)
    }
    pub fn evaluate_application(&self, context: &Context) -> Decision {
        let app = &context.application;
        if app.trim().is_empty() || context.process_id <= 0 || context.window_id <= 0 {
            return Decision::UnknownApplication;
        }
        let matches_app = |rule: &Rule| matches!(rule,Rule::App{bundle_id} if bundle_id.eq_ignore_ascii_case(app));
        let o = &self.observation;
        if o.blocklist.iter().any(matches_app) {
            return Decision::ApplicationBlocked;
        }
        if o.default_application_behavior == Behavior::DoNotObserve
            && !o.allowlist.iter().any(matches_app)
        {
            return Decision::ApplicationBlocked;
        }
        Decision::Allowed
    }
    fn evaluate_website(&self, website: &Website, o: &Observation) -> Decision {
        let host = match website {
            Website::NotWeb => return Decision::Allowed,
            Website::Unknown => {
                return if o.default_url_behavior == Behavior::Observe
                    && !o.blocklist.iter().any(|r| matches!(r, Rule::Url { .. }))
                {
                    Decision::Allowed
                } else {
                    Decision::UnknownWebsite
                }
            }
            Website::Url(value) => match web_host(value) {
                Some(host) => host,
                None => return Decision::UnknownWebsite,
            },
        };
        let matches_url =
            |rule: &Rule| matches!(rule,Rule::Url{url_domain} if domain_matches(&host,url_domain));
        if o.blocklist.iter().any(matches_url) {
            return Decision::WebsiteBlocked;
        }
        if o.default_url_behavior == Behavior::DoNotObserve && !o.allowlist.iter().any(matches_url)
        {
            return Decision::WebsiteBlocked;
        }
        Decision::Allowed
    }
}
pub fn domain(value: &str) -> Result<String> {
    if value.is_empty()
        || value.len() > 1024
        || value.trim() != value
        || value.contains(['/', '\\', '@', '?', '#', '%'])
    {
        return Err(Error::invalid(
            "Website rules need a domain without a scheme, credentials, path or port",
        ));
    }
    let value = value.strip_suffix('.').unwrap_or(value);
    let host = Host::parse(value).map_err(|_| Error::invalid("Invalid website domain"))?;
    match host {
        Host::Domain(ref d) => {
            if d.len() > 253
                || d.split('.').any(|s| {
                    s.is_empty()
                        || s.len() > 63
                        || s.starts_with('-')
                        || s.ends_with('-')
                        || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                })
            {
                return Err(Error::invalid("Invalid website domain"));
            }
        }
        Host::Ipv4(_) | Host::Ipv6(_) => {}
    }
    Ok(host.to_string().to_ascii_lowercase())
}
pub fn web_host(value: &str) -> Option<String> {
    let url = Url::parse(value).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    domain(url.host()?.to_string().as_str()).ok()
}
fn domain_matches(host: &str, rule: &str) -> bool {
    host == rule
        || host
            .strip_suffix(rule)
            .is_some_and(|prefix| prefix.ends_with('.'))
}
pub fn browser(application: &str) -> bool {
    let app = application.strip_suffix(" (deleted)").unwrap_or(application).to_ascii_lowercase();
    [
        "com.apple.safari",
        "com.google.chrome",
        "com.microsoft.edgemac",
        "org.mozilla.firefox",
        "company.thebrowser.browser",
        "com.brave.browser",
        "com.operasoftware.opera",
        "org.chromium.chromium",
        "com.vivaldi.vivaldi",
    ]
    .iter()
    .any(|name| app == *name || app.starts_with(&format!("{name}.")))
        || [
            "chrome",
            "chrome.exe",
            "chromium",
            "chromium-browser",
            "google-chrome",
            "google-chrome-stable",
            "google-chrome-beta",
            "google-chrome-unstable",
            "firefox-esr",
            "firefox-bin", "waterfox", "floorp", "thorium-browser", "zen-bin",
            "microsoft-edge-beta", "microsoft-edge-dev",
            "brave-browser",
            "microsoft-edge",
            "microsoft-edge-stable",
            "vivaldi-bin",
            "epiphany",
            "falkon",
            "qutebrowser",
            "tor-browser",
            "librewolf",
            "zen",
            "firefox",
            "firefox.exe",
            "msedge",
            "msedge.exe",
            "safari",
            "brave",
            "brave.exe",
            "vivaldi",
            "opera",
        ]
        .contains(&app.as_str()) || app.starts_with("opera-")
}
