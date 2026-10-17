//! The prompts are the shipped bytes, and the structural contract the code
//! enforces is the one the summarizer prompt states (headings, frontmatter
//! fields, file-name format, the resources folder, the phase2 tag).
use skysight::*;
use std::path::PathBuf;

fn repo_root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap() }

#[test]
fn prompts_are_byte_identical_to_the_corpus() {
    let s = std::fs::read(repo_root().join("codex-corpus/mac/ComputerUse/SkysightSummarizer.md")).unwrap();
    let m = std::fs::read(repo_root().join("codex-corpus/mac/ComputerUse/SkysightMemoryInstructions.md")).unwrap();
    assert_eq!(SUMMARIZER_PROMPT.as_bytes(), &s[..]);
    assert_eq!(MEMORY_INSTRUCTIONS.as_bytes(), &m[..]);
}

#[test]
fn contract_constants_are_the_prompt_text() {
    // headings, in the order the prompt lists them
    let pos = |h: &str| SUMMARIZER_PROMPT.find(&format!("\n{h}\n")).unwrap_or_else(|| panic!("prompt lacks heading {h:?}"));
    assert!(pos(H_MEMORY) < pos(H_PRIOR) && pos(H_PRIOR) < pos(H_NONOBVIOUS) && pos(H_NONOBVIOUS) < pos(H_RECORDING) && pos(H_RECORDING) < pos(H_CITATIONS));
    // frontmatter example in the prompt has the fields in our order
    let fm = SUMMARIZER_PROMPT.split("```yaml\n---\n").nth(1).unwrap().split("\n---\n").next().unwrap();
    let keys: Vec<&str> = fm.lines().filter(|l| !l.starts_with(' ')).map(|l| l.split(':').next().unwrap()).collect();
    assert_eq!(keys, ["title", "description", "applications", "suggestion"]);
    let sub: Vec<&str> = fm.lines().filter(|l| l.starts_with("  ")).map(|l| l.trim().split(':').next().unwrap()).collect();
    assert_eq!(sub, ["type", "name", "description"]);
    // the memory instructions pin the folder, the file-name formats and the tag
    assert!(MEMORY_INSTRUCTIONS.contains("`YYYY-MM-DDTHH-MM-SS-{4_alpha_chars}-10min-{slug_description}.md`"));
    assert!(MEMORY_INSTRUCTIONS.contains("`YYYY-MM-DDTHH-MM-SS-{4_alpha_chars}-6h-{slug_description}.md`"));
    assert!(MEMORY_INSTRUCTIONS.contains(&format!("Include the tag `{PHASE2_TAG}`")));
    assert!(SUMMARIZER_PROMPT.contains(&format!("`$HOME/{RESOURCES_REL}/`")));
    // the cadences are the two the prompts name
    assert_eq!((Cadence::TenMin.token(), Cadence::SixHour.token()), ("10min", "6h"));
    assert!(SUMMARIZER_PROMPT.contains("For `10min`") && SUMMARIZER_PROMPT.contains("For `6h`"));
    // and the descriptive form the prompt gives as the example
    assert!(SUMMARIZER_PROMPT.contains("\"The user ran `x` to do `y`\""));
    assert!(observed_command("x").starts_with("The user ran `x`"));
}
