#[cfg(target_os = "linux")]
fn main() {
    use computer_history::{capture::Source, store::CaptureMode};
    assert_eq!(std::env::var("ICU_DISPOSABLE_HOST").as_deref(), Ok("1"));
    let expected =
        std::env::var("ICU_HISTORY_PROBE_APP").expect("Owned fixture executable required");
    assert!(expected.starts_with("icu-history-"));
    let mode = match std::env::var("ICU_HISTORY_PROBE_MODE").as_deref() {
        Ok("semantic") => CaptureMode::Semantic,
        Ok("values") | Err(_) => CaptureMode::Values,
        _ => panic!("Unsupported probe mode"),
    };
    let mut source = computer_history::linux::LinuxSource::default();
    let Some(context) = source.identify().expect("Identify native fixture") else {
        println!("{{\"captured\":false}}");
        return;
    };
    if context.application != expected {
        println!("{{\"captured\":false,\"reason\":\"different application\"}}");
        return;
    }
    let value = source.read(&context, mode).expect("Read opted-in fixture");
    if let Ok(delay) = std::env::var("ICU_HISTORY_VERIFY_DELAY_MS") {
        let delay: u64 = delay.parse().expect("Bounded verification delay");
        assert!(delay <= 5000);
        eprintln!("snapshot_ready");
        std::thread::sleep(std::time::Duration::from_millis(delay));
    }
    let valid = source.verify(&context).expect("Verify fixture");
    println!(
        "{}",
        serde_json::json!({"application":context.application,"process_id":context.process_id,"window_id":context.window_id,"content":if valid {value}else{None},"verified":valid})
    );
    drop(source);
}
#[cfg(not(target_os = "linux"))]
fn main() {
    panic!("Run this probe only in an owned Linux fixture");
}
