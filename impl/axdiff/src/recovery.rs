//! Semantic target recovery reconstructed from the current native service.
//! See study/audit/native-refetch-2026-10-06/property-map.json for the witness slots.
//! This matcher never establishes window ownership or authorizes an action.

/// Exact optional values, without trimming, placeholder substitution, or fuzzy matching.
/// Adapters must distinguish an absent attribute from an unreadable observation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fingerprint {
    pub role: String,
    pub subrole: Option<String>,
    pub role_description: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub value: Option<String>,
    pub value_description: Option<String>,
    pub placeholder: Option<String>,
    pub help: Option<String>,
    pub identifier: Option<String>,
    pub url: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal { Incomplete, MissingBefore, AmbiguousBefore, MissingAfter, AmbiguousAfter }

impl Fingerprint {
    pub fn matches(&self, other: &Self, ignore_value_change: bool) -> bool {
        self.role == other.role && self.subrole == other.subrole
            && self.role_description == other.role_description && self.title == other.title
            && self.description == other.description
            && (ignore_value_change || self.value == other.value)
            && self.value_description == other.value_description && self.placeholder == other.placeholder
            && self.help == other.help && self.identifier == other.identifier && self.url == other.url
    }
}

/// The old target must already have been unique. Uniqueness gained by deleting a
/// duplicate is not identity evidence. Incomplete trees cannot establish uniqueness.
pub fn recover(target: &Fingerprint, before: &[Fingerprint], after: &[Fingerprint],
               complete: bool, ignore_value_change: bool) -> Result<usize, Refusal> {
    if !complete { return Err(Refusal::Incomplete); }
    match before.iter().filter(|candidate| target.matches(candidate, false)).count() {
        0 => return Err(Refusal::MissingBefore),
        1 => (),
        _ => return Err(Refusal::AmbiguousBefore),
    }
    let mut matches = after.iter().enumerate().filter(|(_, candidate)| target.matches(candidate, ignore_value_change));
    let index = matches.next().ok_or(Refusal::MissingAfter)?.0;
    if matches.next().is_some() { return Err(Refusal::AmbiguousAfter); }
    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn field() -> Fingerprint { Fingerprint { role: "AXTextField".into(), title: Some("Name".into()), value: Some("old".into()), ..Default::default() } }
    #[test]
    fn recovery_requires_unique_before_and_after() {
        let target = field();
        assert_eq!(recover(&target, &[target.clone()], &[target.clone()], true, false), Ok(0));
        assert_eq!(recover(&target, &[], &[target.clone()], true, false), Err(Refusal::MissingBefore));
        assert_eq!(recover(&target, &[target.clone(), target.clone()], &[target.clone()], true, false), Err(Refusal::AmbiguousBefore));
        assert_eq!(recover(&target, &[target.clone()], &[], true, false), Err(Refusal::MissingAfter));
        assert_eq!(recover(&target, &[target.clone()], &[target.clone(), target.clone()], true, false), Err(Refusal::AmbiguousAfter));
        assert_eq!(recover(&target, &[target.clone()], &[target.clone()], false, false), Err(Refusal::Incomplete));
    }
    #[test]
    fn recovery_value_tolerance_is_only_for_the_new_state() {
        let target = field(); let mut edited = target.clone(); edited.value = Some("new".into());
        assert_eq!(recover(&target, &[target.clone()], &[edited.clone()], true, false), Err(Refusal::MissingAfter));
        assert_eq!(recover(&target, &[target.clone()], &[edited.clone()], true, true), Ok(0));
        assert_eq!(recover(&target, &[edited.clone()], &[edited], true, true), Err(Refusal::MissingBefore));
    }
    #[test]
    fn recovery_checks_every_recovered_property_even_after_an_edit() {
        let target = field();
        let mutations: Vec<Box<dyn Fn(&mut Fingerprint)>> = vec![
            Box::new(|x| x.role.push('x')), Box::new(|x| x.subrole = Some("x".into())),
            Box::new(|x| x.role_description = Some("x".into())), Box::new(|x| x.title = None),
            Box::new(|x| x.description = Some("x".into())), Box::new(|x| x.value_description = Some("x".into())),
            Box::new(|x| x.placeholder = Some("x".into())), Box::new(|x| x.help = Some("x".into())),
            Box::new(|x| x.identifier = Some("x".into())), Box::new(|x| x.url = Some("https://example.test".into())),
        ];
        for mutate in mutations {
            let mut changed = target.clone(); mutate(&mut changed);
            assert_eq!(recover(&target, &[target.clone()], &[changed], true, true), Err(Refusal::MissingAfter));
        }
    }
    #[test]
    fn recovery_does_not_trim_or_merge_absent_and_empty_attributes() {
        let target = field(); let mut other = target.clone(); other.title = Some(" Name ".into());
        assert!(!target.matches(&other, false));
        other = target.clone(); other.help = Some(String::new());
        assert!(!target.matches(&other, false));
    }
}
