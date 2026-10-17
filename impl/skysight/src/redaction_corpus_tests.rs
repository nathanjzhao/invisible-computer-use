use super::redact_command;

#[test]
fn ordinary_corpus_audit() {
    for (i,line) in include_str!("../tests/data/reviewed-ordinary.txt").lines().enumerate() {
        let command=line.replace("\\n","\n");
        println!("ORDINARY {}",serde_json::json!({"index":i,"command":command,"redacted":redact_command(&command)}));
    }
}

#[test]
fn reviewed_credential_corpus_has_no_retained_secret() {
    // The public synthetic corpus from review round 6, retained without edits.
    let mut leaks=Vec::new();
    for line in include_str!("../tests/data/reviewed-secrets.tsv").lines() {
        let (secret,command)=line.split_once('\t').unwrap();
        let command=command.replace("\\n","\n");
        let result=redact_command(&command);
        if result.contains(secret) {leaks.push((command,result));}
    }
    assert!(leaks.is_empty(),"Unredacted credential cases: {leaks:#?}");
}

#[test]
fn short_credentials_are_hidden_by_position_without_masking_normal_flags() {
    for command in [
        "zip -Porchid archive.zip note.txt", "unzip -P 'orchid petal' archive.zip",
        "7z a -porchid archive.7z note.txt", "sqlcmd -S db -P orchid", "ldapsearch -x -w orchid",
        "twine upload -p orchid dist/*", "smbclient //h/share -Ubob%orchid",
        "poetry config http-basic.repo user orchid", "poetry config pypi-token.repo orchid",
        "nmcli dev wifi connect Fixture password orchid", "wpa_passphrase Fixture orchid",
        "net user fixture orchid /add", "cmdkey /add:fixture /user:fixture /pass:orchid",
        r#"C:\Windows\System32\cmdkey.exe /add:fixture /pass:orchid"#,
        r#"psql -c "ALTER USER fixture WITH PASSWORD 'orchid petal'""#,
        r#"Invoke-RestMethod -Headers @{Authorization="Bearer orchid"}"#,
        r#"expect -c 'spawn ssh fixture; expect "password:"; send "orchid\r"'"#,
    ] {assert!(!redact_command(command).contains("orchid"),"{command}");}
    for command in [
        "zip -r archive.zip note.txt", "unzip -l archive.zip", "7z l archive.7z", "7z a -p archive.7z",
        "sqlcmd -S db -E", "ldapsearch -x -W", "twine --help", "smbclient //h/share -U bob",
        "poetry config virtualenvs.create false", "poetry config http-basic.repo", "nmcli dev wifi list",
        "wpa_passphrase Fixture", "net user fixture /domain", "cmdkey /list", "grep PASSWORD notes.txt",
        r#"expect -c 'spawn echo hi; expect "ready"; send "hello\r"'"#,
    ] {assert_eq!(redact_command(command),command,"ordinary command changed");}
}

#[test]
fn credential_arguments_named_like_tools_are_not_reinterpreted_as_programs() {
    for (command,expected) in [
        ("wpa_passphrase zip mysql","wpa_passphrase zip <redacted>"),
        ("net user docker zip /add","net user docker <redacted> /add"),
        ("poetry config http-basic.repo mysql zip","poetry config http-basic.repo mysql <redacted>"),
        ("poetry config pypi-token.repo zip","poetry config pypi-token.repo <redacted>"),
        ("poetry run twine upload -p orchid dist/*","poetry run twine upload -p <redacted> dist/*"),
        ("fixture$ psql -c \"ALTER USER app PASSWORD 'orchid'\"","fixture$ psql -c \"ALTER USER app PASSWORD '<redacted>'\""),
    ] {assert_eq!(redact_command(command),expected);}
}

#[test]
fn repeated_command_redaction_preserves_markers_inside_arguments() {
    for command in ["smbclient //fixture/share -U user%virella", "smbclient //fixture/share -Uuser%virella", "curl -u user:virella https://example.test", "zip -P virella archive.zip note.txt"] {
        let once = redact_command(command);
        assert!(!once.contains("virella"));
        assert_eq!(redact_command(&once), once, "second pass changed marker: {command}");
    }
}
