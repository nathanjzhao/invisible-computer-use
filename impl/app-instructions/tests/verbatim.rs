//! The pack CONTENT must be the shipped bytes, not a paraphrase: every embedded
//! pack equals the corpus copy (`codex-corpus/mac/ComputerUse/AppInstructions/`)
//! and the conformance golden (`conformance/golden/app-instructions/`), and the
//! set of files is exactly the seven that ship.
use app_instructions::{pack_for_bundle_id, PACKS};
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

#[test]
fn every_pack_is_byte_identical_to_corpus_and_golden() {
    let corpus = repo_root().join("codex-corpus/mac/ComputerUse/AppInstructions");
    let golden = repo_root().join("conformance/golden/app-instructions");
    for p in PACKS {
        let c = fs::read(corpus.join(p.file)).unwrap_or_else(|e| panic!("{}: {e}", p.file));
        let g = fs::read(golden.join(p.file)).unwrap_or_else(|e| panic!("{}: {e}", p.file));
        assert_eq!(p.body.as_bytes(), &c[..], "{} differs from the corpus copy", p.file);
        assert_eq!(p.body.as_bytes(), &g[..], "{} differs from the golden copy", p.file);
    }
}

#[test]
fn the_seven_shipped_files_and_no_others() {
    let corpus = repo_root().join("codex-corpus/mac/ComputerUse/AppInstructions");
    let on_disk: BTreeSet<String> = fs::read_dir(&corpus).unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".md")).collect();
    let embedded: BTreeSet<String> = PACKS.iter().map(|p| p.file.to_string()).collect();
    assert_eq!(embedded, on_disk);
    assert_eq!(embedded.len(), 7);
}

#[test]
fn embedded_packs_hash_to_the_shipped_sha256sums_manifest() {
    // reference/openai-sky/app-bundle-packs/SHA256SUMS.txt was recorded from the
    // live ChatGPT.app bundle. Hash every embedded body (via the system shasum so
    // no crypto dependency is needed) and compare to the manifest line for it.
    let m = fs::read_to_string(repo_root().join("reference/openai-sky/app-bundle-packs/SHA256SUMS.txt")).unwrap();
    let manifest: std::collections::BTreeMap<String, String> = m.lines()
        .filter_map(|l| l.split_once("  ").map(|(h, f)| (f.to_string(), h.to_string()))).collect();
    let embedded: BTreeSet<String> = PACKS.iter().map(|p| p.file.to_string()).collect();
    assert_eq!(manifest.keys().cloned().collect::<BTreeSet<_>>(), embedded);
    for p in PACKS {
        use std::io::Write;
        let mut child = std::process::Command::new("shasum").args(["-a", "256"])
            .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(p.body.as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        let hash = String::from_utf8(out.stdout).unwrap().split_whitespace().next().unwrap().to_string();
        assert_eq!(hash, manifest[p.file], "{} does not hash to the shipped bytes", p.file);
    }
    // and each bundle id in the table round-trips to its own file
    for p in PACKS {
        assert_eq!(pack_for_bundle_id(p.bundle_id).unwrap().file, p.file);
    }
}
