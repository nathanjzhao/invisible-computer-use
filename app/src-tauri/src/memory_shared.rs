//! Review a prior preference using only a quote approved in the current task.
//! A local version fingerprint binds retirement; it is never a model payload.
use super::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SharedPreference {
    pub entry_id: String,
    pub entry_sha256: String,
    pub quote: String,
    pub goal: String,
}

pub(super) fn fingerprint(entry: &Entry) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(serde_json::to_vec(entry).unwrap()))
}

fn in_scope(entry: &Entry, app: &str) -> bool {
    entry.scope == "global" || entry.scope.eq_ignore_ascii_case(app)
}

fn validated_quote(entry: &Entry, summaries: &BTreeMap<String, ChatSummary>) -> Option<String> {
    if entry.untrusted || !entry.judge_answered || !about_user(entry.kind)
        || entry.id != entry_id(&entry.scope, &entry.text) || entry.evidence.is_empty() {
        return None;
    }
    let evidence: Vec<_> = entry.evidence.iter().map(|id| summaries.get(id)).collect::<Option<_>>()?;
    if entry.scope != "global" && evidence.iter().any(|s| !s.app_key().eq_ignore_ascii_case(&entry.scope)) {
        return None;
    }
    if observed(entry.kind, &entry.scope, &entry.user_said, &evidence) { return None; }
    let quote = keep_text(user_words(entry.kind, &entry.scope, &entry.user_said, &evidence)?.join(" / ").trim());
    // A clipped quote may have lost its scope or exception. It grants no review
    // authority even though the ordinary context renderer can display it.
    if quote.is_empty() || quote.chars().count() > MAX_SAID_CHARS || addresses_judge(&quote) { return None; }
    Some(quote)
}

pub(super) fn capture(entry: &Entry, rendered_quote: &str, app: &str, goal: &str, started: Ts,
                     summaries: &BTreeMap<String, ChatSummary>) -> Option<SharedPreference> {
    if !in_scope(entry, app) || goal.is_empty() || goal.chars().count() > 600 { return None; }
    let quote = validated_quote(entry, summaries)?;
    if quote != keep_text(rendered_quote.trim())
        || entry.evidence.iter().any(|id| summaries.get(id).is_none_or(|s| s.ended > started)) {
        return None;
    }
    Some(SharedPreference { entry_id: entry.id.clone(), entry_sha256: fingerprint(entry), quote, goal: goal.to_owned() })
}

pub(super) fn sanitize(grants: &mut Vec<SharedPreference>, original_goal: &str, safe_goal: &str, spans: &[String]) {
    // A redacted or truncated correction may no longer express the same scope.
    // Never repair it by borrowing words from a remembered preference.
    if original_goal != safe_goal || safe_goal.is_empty() || safe_goal.chars().count() > 600 {
        grants.clear();
        return;
    }
    grants.retain(|grant| grant.goal == safe_goal
        && hide_spans(&grant.quote, spans) == grant.quote);
}

pub(super) struct CrossPreference {
    pub entry: Entry,
    quote: String,
    // Time, own-provider summary id, app, redacted goal, new this pass.
    // No goal from the earlier provider is included.
    goals: Vec<(Ts, String, String, String, bool)>,
}

fn provider(name: &str) -> &str { if name == "claude" { "claude-code" } else { name } }

pub(super) fn candidates(new: &[ChatSummary], snapshot: &BTreeMap<String, Entry>, own: &[Entry],
                         choice: &str, summaries: &BTreeMap<String, ChatSummary>) -> Vec<CrossPreference> {
    if matches!(choice, "" | "auto") { return vec![]; }
    let own: BTreeSet<_> = own.iter().map(|entry| entry.id.as_str()).collect();
    let new_ids: BTreeSet<_> = new.iter().map(|summary| summary.id.as_str()).collect();
    let mut ordered: Vec<_> = new.iter().collect();
    ordered.sort_by(|a, b| b.ended.cmp(&a.ended).then_with(|| b.id.cmp(&a.id)));
    let mut found: BTreeMap<String, CrossPreference> = BTreeMap::new();
    for summary in ordered {
        // A different named memory provider was not the recipient of the
        // task's recalled quote. Its ordinary provider policy still applies.
        if provider(&summary.brain_choice) != provider(choice) || !summary.recordings.is_empty() { continue; }
        for grant in &summary.shared_preferences {
            let Some(old) = snapshot.get(&grant.entry_id) else { continue; };
            if own.contains(old.id.as_str()) || fingerprint(old) != grant.entry_sha256
                || !in_scope(old, &summary.app_key()) || !summary.goals.contains(&grant.goal)
                || grant.goal.is_empty() || grant.goal.chars().count() > 600
                || validated_quote(old, summaries).as_deref() != Some(grant.quote.as_str()) {
                continue;
            }
            let candidate = found.entry(old.id.clone()).or_insert_with(|| CrossPreference {
                entry: old.clone(), quote: grant.quote.clone(), goals: vec![],
            });
            let after = old.evidence.iter().filter_map(|id| summaries.get(id)).map(|s| s.ended).max().unwrap();
            let app = summary.app_key();
            let mut later: Vec<_> = summaries.values().filter(|s| provider(&s.brain_choice) == provider(choice)
                && s.recordings.is_empty() && s.app_key().eq_ignore_ascii_case(&app) && s.ended > after).collect();
            later.sort_by(|a, b| b.ended.cmp(&a.ended).then_with(|| b.id.cmp(&a.id)));
            let mut added = 0;
            for source in later {
                for goal in source.goals.iter().rev() {
                    let goal = (source.ended, source.id.clone(), source.app_key(), scrub_cleared(goal, &source.cleared()), new_ids.contains(source.id.as_str()));
                    if !candidate.goals.contains(&goal) { candidate.goals.push(goal); }
                    added += 1;
                    if added >= MAX_LATER_MESSAGES { break; }
                }
                if added >= MAX_LATER_MESSAGES { break; }
            }
            candidate.goals.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
            candidate.goals.truncate(MAX_LATER_MESSAGES);
        }
    }
    found.into_values().filter(|candidate| !candidate.goals.is_empty()).collect()
}

pub(super) fn still_current(candidate: &CrossPreference, entries: &[Entry], summaries: &BTreeMap<String, ChatSummary>) -> bool {
    entries.iter().find(|entry| entry.id == candidate.entry.id).is_some_and(|entry|
        entry == &candidate.entry && validated_quote(entry, summaries).as_deref() == Some(candidate.quote.as_str()))
}

pub(super) fn input(candidates: &[CrossPreference]) -> (Value, Vec<JudgeMessage>) {
    let mut messages = Vec::new();
    let mut shown = Vec::new();
    let mut proposed = Vec::new();
    for (index, candidate) in candidates.iter().enumerate() {
        let scope = if candidate.entry.scope == "global" { "all apps" } else { &candidate.entry.scope };
        let quote_id = format!("m{}", messages.len() + 1);
        shown.push(json!({"id":quote_id,"app":scope,"text":candidate.quote}));
        messages.push((format!("shared:{}", candidate.entry.id), candidate.quote.clone(), false));
        let mut later = Vec::new();
        for (_, summary, app, goal, new) in &candidate.goals {
            let id = format!("m{}", messages.len() + 1);
            shown.push(json!({"id":id,"app":app,"text":goal}));
            later.push(id);
            messages.push((summary.clone(), goal.clone(), *new));
        }
        // The task received this user quote, not the complete older paraphrase
        // or its source record. Judge only the quoted preference's scope.
        proposed.push(json!({"id":format!("c{}",index+1),"kind":candidate.entry.kind,"scope":scope,
            "text":candidate.quote,"rests_on":[quote_id],"later":later}));
    }
    (json!({"messages":shown,"candidates":proposed}), messages)
}
