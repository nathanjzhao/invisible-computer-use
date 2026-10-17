#[cfg(target_os = "linux")]
fn main() {
    use computer_history::{capture::Source, linux::LinuxSource};
    assert_eq!(std::env::var("ICU_DISPOSABLE_HOST").as_deref(), Ok("1"));
    let expected: i64 = std::env::var("ICU_HISTORY_TERMINAL_PID")
        .expect("Owned terminal PID required")
        .parse()
        .unwrap();
    let mut source = LinuxSource::default();
    let context = source
        .identify()
        .expect("Identify")
        .expect("Active unlocked fixture");
    assert_eq!(context.application, "xfce4-terminal");
    assert_eq!(context.process_id, expected);
    let content = source.terminal_text(&context).expect("Terminal read");
    assert!(
        source.native_window_text(&context).unwrap().is_none(),
        "No ordinary terminal snapshot"
    );
    assert!(
        source
            .read(&context, computer_history::store::CaptureMode::Values)
            .unwrap()
            .is_none(),
        "No terminal focused-value projection"
    );
    let verified = source.verify(&context).expect("Recheck identity");
    println!(
        "{}",
        serde_json::json!({"scope":"native component, synthetic terminal only", "verified":verified,"content":if verified {content} else {None}})
    );
}
#[cfg(not(target_os = "linux"))]
fn main() {
    panic!("Owned Linux fixture only")
}
