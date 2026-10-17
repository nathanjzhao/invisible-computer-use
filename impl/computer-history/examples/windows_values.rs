//! Explicit, disposable Windows fixture probe. Not a production recorder.
#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use computer_history::{capture::Source, policy::{Behavior,Rule},store::{CaptureMode,Store},windows::{Invocations,WindowsSource}};
    use std::{path::PathBuf,time::{Duration,Instant}};
    assert_eq!(std::env::var("ICU_DISPOSABLE_HOST").as_deref(),Ok("1"));
    assert_eq!(std::env::var("USERNAME").as_deref(),Ok("ICUAudit"));
    assert_eq!(std::fs::read_to_string("C:/ICUAudit/owner.txt")?,"icu-parity25-20261015-071103");
    let args:Vec<_>=std::env::args().collect();assert_eq!(args.len(),4);
    let pid:i64=args[1].parse()?;let window:i64=args[2].parse()?;let root=PathBuf::from(&args[3]);
    assert!(root.starts_with("C:/ICUAudit/round25/acceptance")&&!root.exists());
    let store=Store::open(root)?;let mut settings=store.state()?.settings;
    settings.observation.default_application_behavior=Behavior::DoNotObserve;
    settings.observation.allowlist.push(Rule::App{bundle_id:"HistoryFixture.exe".into()});
    store.update(settings,None)?;store.configure(true,7,true)?;
    let state=store.state()?;store.capture_mode(CaptureMode::InteractionEvents,true,(&state.generation,state.revision))?;
    let _collector=store.attach_collector("native-provider-values-component")?;
    let mut source=WindowsSource::default();let context=source.identify()?.ok_or("No foreground fixture")?;
    assert_eq!((context.process_id,context.window_id,context.application.as_str()),(pid,window,"HistoryFixture.exe"));
    let state=store.state()?;let mut events=Invocations::start_with_values(&context,&store,(&state.generation,state.revision))?;
    let mut rows=Vec::new();let end=Instant::now()+Duration::from_secs(85);let mut partial=false;
    while Instant::now()<end {
        let (values,lost)=events.drain_values(0);partial|=lost;
        for value in values {
            let row=serde_json::json!({"source_timestamp_ms":value.ticket.timestamp(),"label":value.content.label,"value":value.content.value,"observed_at_ms":computer_history::now_ms()});
            store.commit(value.ticket,&context,"ui.value",row.clone())?;rows.push(row);
        }
        #[cfg(debug_assertions)] let event_counters=events.value_diagnostics();
        #[cfg(not(debug_assertions))] let event_counters:Vec<usize>=Vec::new();
        std::fs::write(store.root().join("component-values.json"),serde_json::to_vec_pretty(&serde_json::json!({"rows":rows,"partial":partial,"event_counters":event_counters}))?)?;
        std::thread::sleep(Duration::from_millis(60));
    }
    Ok(())
}
#[cfg(not(windows))]
fn main() { eprintln!("This probe requires an owned Windows fixture"); }
