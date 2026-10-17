// Component probe for the Linux lane's verified AT-SPI/XInput2 collector.
#[cfg(target_os = "linux")]
fn main() {
    use computer_history::{capture::Source, linux::LinuxSource, linux_input::{Input, InputSource}};
    assert_eq!(std::env::var("ICU_DISPOSABLE_HOST").as_deref(), Ok("1"));
    let expected = std::env::var("ICU_HISTORY_PROBE_APP").expect("Owned fixture required");
    assert!(expected.starts_with("icu-history-"));
    let mut source = LinuxSource::default();
    let context = source.identify().expect("Identify").expect("Unlocked active fixture");
    assert_eq!(context.application, expected);
    let snapshot = source.native_window_text(&context).expect("Snapshot");
    let input = InputSource::start(&mut source, &context).expect("Input observation");
    eprintln!("input_ready");
    let mut rows = Vec::new();
    let mut lost = false;
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < until {
        let (events, dropped) = input.drain().expect("Bound input scope");
        lost |= dropped;
        for event in events {
            let action = match event.input {
                Input::Click { right } => serde_json::json!({"click":true,"right":right}),
                Input::Key { name, ctrl, alt, shift, win } =>
                    serde_json::json!({"key":name,"ctrl":ctrl,"alt":alt,"shift":shift,"meta":win}),
            };
            rows.push(serde_json::json!({"action":action,"timestamp_ms":event.timestamp_ms}));
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let before = std::time::Instant::now();
    drop(input);
    println!("{}", serde_json::json!({"scope":"native component, not signed app evidence",
        "snapshot":snapshot,"events":rows,"lost":lost,"stop_ms":before.elapsed().as_millis()}));
}
#[cfg(not(target_os = "linux"))]
fn main() { panic!("Owned Linux fixture only"); }
