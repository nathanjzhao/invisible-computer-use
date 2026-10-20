//! Tauri backend: exposes the cua-driver "hands" to the webview UI.
mod agent;
mod action_failure;
mod brain;
mod onboarding;
mod vision;
mod audio;
mod audio_api;
mod qt5_compat;
pub use qt5_compat::entry as qt5_compat_entry;
pub use audio_api::entry as audio_api_entry;
mod audio_wav;
#[cfg(target_os = "macos")]
mod audio_macos;
mod audio_supervisor;
pub use audio_supervisor::entry as audio_supervisor_entry;
mod history;
#[cfg(target_os = "linux")]
mod linux_helpers;
#[cfg(target_os = "macos")]
mod history_events;
#[cfg(target_os = "windows")]
mod history_events_windows;
#[cfg(target_os = "linux")]
mod history_events_linux;
#[cfg(test)]
mod claude_code_bridge;
mod cua;
mod frame_serve;
mod elevated_update;
mod gate;
mod linux_pkg;
mod memory;
mod trajectory;
mod task_progress;
mod oauth;
mod permissions;
mod subprocess;
mod transport;
#[cfg(test)]
mod updater_tests;
mod run_control;
mod fleet;

/// Tests that re-run this test binary as a child each start an owned fake
/// driver, which has the product's 3 s startup deadline. Run those children
/// one at a time so parallel test threads cannot starve that deadline.
#[cfg(test)]
pub(crate) fn child_test_output(command: &mut std::process::Command, timeout: std::time::Duration)
    -> Result<std::process::Output, String> {
    static CHILD_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _slot = CHILD_TESTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    // All users of this helper start synthetic local fixture drivers. The
    // override is compiled out of production permission checks.
    if !command.get_envs().any(|(key, _)| key == "ICU_TEST_IO_PERMISSIONS") {
        command.env("ICU_TEST_IO_PERMISSIONS", "granted");
    }
    subprocess::output(command, timeout)
}

use serde_json::Value;
use tauri::Manager;
use run_control::{RunInfo, RunManager};

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn agent_run<R: tauri::Runtime>(app: tauri::AppHandle<R>, goal: String, pid: i64, window_id: i64, app_name: String, brain: Option<String>, run_id: Option<String>,
                                use_memory: Option<bool>, remember: Option<bool>, resume_id: Option<String>, decision_budget: Option<usize>, allow_foreground:Option<bool>, use_screenshots:Option<bool>, workflow:Option<memory::workflows::InvocationRequest>) -> Result<String, String> {
    let memory=app.state::<memory::MemoryService>();
    if workflow.is_some() && resume_id.is_some() { return Err("Choose a saved workflow or a paused task".into()); }
    let workflow=workflow.as_ref().map(|request|memory.workflow_load(request)).transpose()?;
    if let Some(workflow)=&workflow {
        let identity=agent::bundle_for_pid(pid).ok_or("The selected target has no stable application identity")?;
        if !workflow.matches_app(&identity) { return Err(format!("Select the recorded {} application before using this workflow",workflow.workflow.app_name)); }
    }
    let resume=resume_id.as_deref().map(|id|memory.checkpoint(id)).transpose()?;
    let goal=if goal.trim().is_empty(){resume.as_ref().map(|r|r.goal.clone()).unwrap_or(goal)}else{goal};
    let budget=decision_budget.unwrap_or(task_progress::DEFAULT_BUDGET);
    if !(1..=task_progress::MAX_BUDGET).contains(&budget){return Err("decision budget must be between 1 and 128".into());}
    if goal.trim().is_empty() {
        return Err("goal must not be empty".into());
    }
    if pid <= 0 || window_id <= 0 {
        return Err("select a valid target window".into());
    }
    let choice = brain.unwrap_or_else(|| "auto".into());
    let id = run_id.unwrap_or_else(|| format!("run-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()));
    // The per-run memory controls travel beside the run, read once by its task.
    app.state::<memory::MemoryService>().set_run_options(&id, memory::RunOptions {
        allow_foreground:allow_foreground.unwrap_or(false),use_screenshots:use_screenshots.unwrap_or(false), use_memory: use_memory.unwrap_or(true), remember: remember.unwrap_or(true), brain_choice: choice.clone() });
    memory.set_task_launch(&id,budget,resume);
    let lease = match app.state::<RunManager>().begin(RunInfo { id: id.clone(), goal, pid, window_id, app_name }) {
        Ok(lease) => lease,
        Err(e) => { app.state::<memory::MemoryService>().take_run_options(&id); memory.take_task_launch(&id); return Err(e); }
    };
    let runner = app.state::<TaskRunner<R>>().0.clone();
    if let Some(workflow)=workflow {app.state::<memory::workflows::Runtime>().set_run(id.clone(),workflow);}
    let worker_app=app.clone();
    if let Err(error)=std::thread::Builder::new().name("computer-task".into()).spawn(move || runner(worker_app, choice, lease)) {
        memory.take_run_options(&id);memory.take_task_launch(&id);
        app.state::<memory::workflows::Runtime>().take_run(&id);
        return Err(format!("could not start task: {error}"));
    }
    Ok(id)
}

/// The task body started for an admitted run. Production always registers
/// `agent::run`; lifecycle tests substitute only the model.
type TaskBody<R> = std::sync::Arc<dyn Fn(tauri::AppHandle<R>, String, run_control::RunLease) + Send + Sync>;
struct TaskRunner<R: tauri::Runtime>(TaskBody<R>);

/// Exit closes task and manual admission, cancels the active run, then stops
/// the owned driver so no queued worker can reach it or relaunch it.
fn close_for_exit<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let _ = app.state::<RunManager>().shutdown();
    if let Some(fleet)=app.try_state::<fleet::Service>() { fleet.shutdown(); }
    if let Some(audio)=app.try_state::<audio::Service>() { audio.shutdown(); }
    if let Some(history)=app.try_state::<history::Service>() { history.shutdown(); }
    // End in-flight CLI and helper trees before waiting for the driver drain.
    subprocess::close();
    cua::shutdown_owned_driver();
}

#[tauri::command]
async fn agent_stop<R: tauri::Runtime>(app: tauri::AppHandle<R>, run_id: String) -> Result<bool, String> {
    let stopped = app.state::<RunManager>().stop(&run_id)?;
    #[cfg(target_os = "linux")]
    if stopped {
        blocking(cua::cancel_pending_portal_setup).await?;
    }
    Ok(stopped)
}

/// The user's answer to a confirmation a running task asked for. False when
/// that task is not the one running or the request is not waiting.
#[tauri::command]
fn agent_confirm<R: tauri::Runtime>(app: tauri::AppHandle<R>, run_id: String, request_id: String, approve: bool) -> Result<bool, String> {
    // A run that is stopping takes no answer: Allow after Stop must not run the step.
    let active = app.state::<RunManager>().status()?.is_some_and(|(info, stopping)| info.id == run_id && !stopping);
    if !active || request_id.contains('/') { return Ok(false); }
    Ok(app.state::<run_control::Confirmations>().answer(&format!("{run_id}/{request_id}"), approve))
}

#[tauri::command]
fn agent_status<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<Value, String> {
    Ok(match app.state::<RunManager>().status()? {
        Some((info, stopping)) => serde_json::json!({
            "runId": info.id, "goal": info.goal, "pid": info.pid,
            "windowId": info.window_id, "appName": info.app_name, "stopping": stopping,
            // The question a reloaded window has to show again.
            "pendingConfirm": if stopping { Value::Null } else { app.state::<run_control::Confirmations>().pending_for(&info.id).unwrap_or(Value::Null) },
        }),
        None => Value::Null,
    })
}

#[tauri::command]
fn agent_checkpoints(state:tauri::State<'_,memory::MemoryService>)->Vec<task_progress::Checkpoint>{state.checkpoints()}
#[tauri::command]
fn agent_checkpoint_discard<R:tauri::Runtime>(app:tauri::AppHandle<R>,id:String)->Result<(),String>{
    if app.state::<RunManager>().status()?.is_some(){return Err("stop the running task before discarding progress".into());}
    app.state::<memory::MemoryService>().delete_checkpoint(&id)
}

#[tauri::command]
fn memory_status<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Value {
    let active = app.state::<RunManager>().status().ok().flatten().is_some();
    app.state::<memory::MemoryService>().status(memory::now(), active)
}

#[tauri::command]
fn computer_history_status(state:tauri::State<'_,history::Service>)->Result<Value,String>{state.status()}
#[tauri::command]
fn computer_history_configure(state:tauri::State<'_,history::Service>,enabled:bool,retention_days:u32,approved:bool)->Result<Value,String>{state.configure(enabled,retention_days,approved)}
#[tauri::command]
fn computer_history_setup_desktop(state:tauri::State<'_,history::Service>,approved:bool,generation:String,revision:u64)->Result<Value,String>{state.setup_desktop(approved,&generation,revision)}
#[tauri::command]
fn computer_history_pause(state:tauri::State<'_,history::Service>,paused:bool)->Result<Value,String>{state.pause(paused)}
#[tauri::command]
fn computer_history_settings_set(state:tauri::State<'_,history::Service>,settings:computer_history::policy::Settings,generation:String,revision:u64)->Result<Value,String>{state.update(settings,&generation,revision)}
#[tauri::command]
fn computer_history_clear<R:tauri::Runtime>(app:tauri::AppHandle<R>)->Result<usize,String>{
    let history=app.state::<history::Service>();
    let store=history.store.as_ref().ok_or("History is unavailable")?;
    store.clear_with(||app.state::<memory::MemoryService>().delete_history_observations(&[],true).map(|_|())
        .map_err(|e|computer_history::Error::new("historySummaryDelete",e))).map_err(|e|e.to_string())
}
#[tauri::command]
fn computer_history_delete_preview<R:tauri::Runtime>(app:tauri::AppHandle<R>,filter:computer_history::store::DeleteFilter,summary:Option<memory::history_content::SummaryKey>)->Result<history::DeletionReview,String>{
    let memories=app.state::<memory::MemoryService>();
    let filter=if let Some(key)=summary.as_ref(){memories.history_summary_filter(key)?}else{filter};
    let raw=app.state::<history::Service>().store.as_ref().ok_or("History is unavailable")?.preview_delete(&filter).map_err(|e|e.to_string())?;
    let summaries=if let Some(key)=summary {vec![key]} else {memories.history_summary_selection(&filter)?};
    let derivatives=memories.history_derivative_selection(&filter)?;
    Ok(history::DeletionReview{generation:raw.generation,revision:raw.revision,events:raw.events,summaries,derivatives})
}
#[tauri::command]
fn computer_history_delete_reviewed<R:tauri::Runtime>(app:tauri::AppHandle<R>,preview:history::DeletionReview,approved:bool)->Result<usize,String>{
    if !approved{return Err("Review and confirm the selected History deletion".into())}
    let history=app.state::<history::Service>();
    history.store.as_ref().ok_or("History is unavailable")?.delete_reviewed(&preview.activity(),|events|
        app.state::<memory::MemoryService>().delete_reviewed_history(events,false,&preview.summaries,&preview.derivatives).map(|_|())
            .map_err(|e|computer_history::Error::new("historySummaryDelete",e))).map_err(|e|e.to_string())
}
#[tauri::command]
fn history_summary_reveal<R:tauri::Runtime>(app:tauri::AppHandle<R>,summary:memory::history_content::SummaryKey)->Result<(),String>{
    let path=app.state::<memory::MemoryService>().history_summary_path(&summary)?;
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(|e|e.to_string())
}
#[tauri::command]
fn history_summary_status<R:tauri::Runtime>(app:tauri::AppHandle<R>)->Result<Value,String>{
    let history=app.state::<history::Service>();
    app.state::<memory::MemoryService>().history_sharing_status(history.store.as_ref().ok_or("History is unavailable")?)
}
#[tauri::command]
fn history_summary_configure<R:tauri::Runtime>(app:tauri::AppHandle<R>,request:memory::history_content::Configure)->Result<Value,String>{
    let history=app.state::<history::Service>();
    app.state::<memory::MemoryService>().history_sharing_configure(history.store.as_ref().ok_or("History is unavailable")?,request)
}
#[tauri::command]
fn history_summary_list<R:tauri::Runtime>(app:tauri::AppHandle<R>)->Result<Vec<memory::history_content::View>,String>{
    let history=app.state::<history::Service>();
    app.state::<memory::MemoryService>().history_summary_list(history.store.as_ref().ok_or("History is unavailable")?)
}
#[tauri::command]
fn computer_history_retention_set(state:tauri::State<'_,history::Service>,retention_days:u32,generation:String,revision:u64)->Result<Value,String>{state.retention(retention_days,&generation,revision)}
#[tauri::command]
fn computer_history_capture_mode(state:tauri::State<'_,history::Service>,mode:computer_history::store::CaptureMode,approved:bool,generation:String,revision:u64)->Result<Value,String>{state.capture_mode(mode,approved,&generation,revision)}
#[tauri::command]
fn computer_history_browser_capture(state:tauri::State<'_,history::Service>,enabled:bool,approved:bool,generation:String,revision:u64)->Result<Value,String>{state.browser_capture(enabled,approved,&generation,revision)}
#[tauri::command]
async fn computer_history_browser_permission(state:tauri::State<'_,history::Service>,application:String)->Result<(),String>{
    let store=state.store.clone().ok_or("History is unavailable")?;
    if !store.state().map_err(|e|e.to_string())?.browser_capture{return Err("Enable browser capture before requesting browser access".into())}
    #[cfg(target_os="macos")]
    return tauri::async_runtime::spawn_blocking(move||computer_history::macos::request_browser_permission(&application).map_err(|e|e.to_string())).await.map_err(|e|e.to_string())?;
    #[cfg(not(target_os="macos"))]
    {let _=application;Err("Browser capture is currently available on macOS only".into())}
}
/// Saves only the fields `settings` names, merged into the file under the store
/// lock, so another app instance's change is kept.
#[tauri::command]
fn memory_settings_set(state: tauri::State<'_, memory::MemoryService>, settings: Value) -> Result<memory::Settings, String> {
    state.update_settings(&settings)
}
/// The pause control: generation stops, stored memories are still used.
#[tauri::command]
fn memory_pause(state: tauri::State<'_, memory::MemoryService>, paused: bool) -> Result<memory::Settings, String> {
    state.set_paused(paused)
}
#[tauri::command]
fn memory_list(state: tauri::State<'_, memory::MemoryService>) -> Value {
    state.list()
}
#[tauri::command]
fn memory_source(state: tauri::State<'_, memory::MemoryService>, key: memory::sources::SourceKey) -> Result<memory::sources::SourceView,String> {
    state.memory_source(&key)
}
#[tauri::command]
fn memory_delete(state: tauri::State<'_, memory::MemoryService>, id: String) -> Result<usize, String> {
    state.delete(&id)
}

#[tauri::command]
async fn trajectory_preview(directory: String, include_values:Option<bool>) -> Result<trajectory::Preview, String> {
    blocking(move || trajectory::preview_with_values(std::path::Path::new(&directory),include_values.unwrap_or(false))).await
}

#[tauri::command]
async fn trajectory_preview_page(request:trajectory::PageRequest)->Result<trajectory::PreviewPage,String>{blocking(move||trajectory::preview_page(&request)).await}

#[tauri::command]
async fn trajectory_import<R: tauri::Runtime>(app: tauri::AppHandle<R>, request: trajectory::ImportRequest) -> Result<Value, String> {
    blocking(move || {
        let memory = app.state::<memory::MemoryService>();
        memory.import_trajectory(&request, memory.models().as_ref())
    }).await
}

#[tauri::command]
async fn workflow_generate<R:tauri::Runtime>(app:tauri::AppHandle<R>,request:memory::workflows::GenerateRequest)->Result<memory::workflows::Generated,String>{
    blocking(move||{
        let provider=request.recording.provider.clone();
        app.state::<memory::workflows::Runtime>().generate(app.state::<memory::MemoryService>().inner(),request,|system,user|{
            brain::make_brain_for(&provider).complete_within(system,user,std::time::Duration::from_secs(120))
        })
    }).await
}
#[tauri::command]
fn workflow_cancel(state:tauri::State<'_,memory::workflows::Runtime>,request_id:String)->bool{state.cancel(&request_id)}
#[tauri::command]
fn workflow_save<R:tauri::Runtime>(app:tauri::AppHandle<R>,request:memory::workflows::SaveRequest)->Result<memory::workflows::Listed,String>{
    let history=app.state::<history::Service>();
    let store=history.store.as_ref().ok_or("History is unavailable")?;
    // I3: a dispatched routine may only be saved with a LIVE `accepted` from the
    // operator. A withdrawn/expired offer answers 409; refuse and discard the draft.
    // (confirm_dispatch_accept is a no-op for a locally-recorded draft.)
    if let Err(e)=fleet::confirm_dispatch_accept(&app,&request.request_id){
        app.state::<memory::workflows::Runtime>().cancel(&request.request_id);
        return Err(e);
    }
    let dispatch_id=app.state::<memory::workflows::Runtime>().draft_dispatch_id(&request.request_id);
    let result=store.with_review_lock(||app.state::<memory::workflows::Runtime>().save(app.state::<memory::MemoryService>().inner(),request)
        .map_err(|e|computer_history::Error::new("workflowSave",e))).map_err(|e|e.to_string());
    // A local save failure after acceptance is reported so the operator sees it.
    if result.is_err() { if let Some(id)=&dispatch_id { fleet::report_local_failure(&app,id); } }
    result
}
#[tauri::command]
fn workflow_list(state:tauri::State<'_,memory::MemoryService>)->Vec<memory::workflows::Listed>{state.workflow_list()}
#[tauri::command]
fn workflow_delete(state:tauri::State<'_,memory::MemoryService>,id:String,expected_sha256:String)->Result<(),String>{state.workflow_delete(&id,&expected_sha256)}

#[tauri::command]
fn audio_status(state:tauri::State<'_,audio::Service>)->Value {state.status()}
#[tauri::command]
fn audio_stop(state:tauri::State<'_,audio::Service>)->Value {state.stop()}
#[tauri::command]
fn audio_start<R:tauri::Runtime>(app:tauri::AppHandle<R>,request:audio::Request)->Result<Value,String>{
    let root=app.path().app_local_data_dir().map_err(|e|e.to_string())?.join("audio");
    app.state::<audio::Service>().start(&root,request)
}

#[tauri::command]
async fn audio_export_wav<R:tauri::Runtime>(app:tauri::AppHandle<R>)->Result<audio_wav::Export,String>{
    let root=app.path().app_local_data_dir().map_err(|e|e.to_string())?.join("audio");
    let status=app.state::<audio::Service>().status();
    blocking(move||audio_wav::export(&root,&status)).await
}

// Driver and capture calls may wait on another process. Always move that work
// off Tauri's event thread, including the first daemon startup.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work).await.map_err(|e| format!("driver worker: {e}"))?
}

#[tauri::command]
async fn cua_status() -> Result<bool, String> {
    blocking(|| cua::ping().map(|_| true)).await
}
/// The configured 1280x820 is taller than a 1280x800 screen's work area (a MacBook Air at its
/// default scale, or macOS 26's VM default), where the window covered everything, Dock included.
/// Shrink to the work area with a margin, and let the minimum give way when the screen is smaller.
fn fitted_size(work: (f64, f64), want: (f64, f64), min: (f64, f64)) -> ((f64, f64), (f64, f64)) {
    let cap = (work.0 * 0.94, work.1 * 0.92);
    let min = (min.0.min(cap.0), min.1.min(cap.1));
    ((want.0.min(cap.0).max(min.0), want.1.min(cap.1).max(min.1)), min)
}

fn fit_main_window(app: &tauri::App) {
    let Some(win) = app.get_webview_window("main") else { return };
    let Ok(Some(monitor)) = win.current_monitor().or_else(|_| win.primary_monitor()) else { return };
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let work = (area.size.width as f64 / scale, area.size.height as f64 / scale);
    let Ok(size) = win.inner_size() else { return };
    let want = (size.width as f64 / scale, size.height as f64 / scale);
    let ((w, h), (mw, mh)) = fitted_size(work, want, (980.0, 640.0));
    if (w, h) == want { return; }
    let _ = win.set_min_size(Some(tauri::LogicalSize::new(mw, mh)));
    let _ = win.set_size(tauri::LogicalSize::new(w, h));
    // `center()` right after `set_size` centers the old size on macOS (seen at 1280x800 @2x:
    // the window landed at the work area's top-left), so place it from the new size.
    let origin = (area.position.x as f64 / scale, area.position.y as f64 / scale);
    let (x, y) = centered(origin, work, (w, h));
    let _ = win.set_position(tauri::LogicalPosition::new(x, y));
}

fn centered(origin: (f64, f64), work: (f64, f64), size: (f64, f64)) -> (f64, f64) {
    (origin.0 + ((work.0 - size.0) / 2.0).max(0.0), origin.1 + ((work.1 - size.1) / 2.0).max(0.0))
}

#[tauri::command]
async fn cua_list_windows() -> Result<Value, String> {
    blocking(cua::list_windows).await
}
#[tauri::command]
async fn cua_window_shot(pid: i64, window_id: i64, max_dim: Option<i64>) -> Result<String, String> {
    blocking(move || cua::window_shot(pid, window_id, max_dim.unwrap_or(1000))).await
}
#[tauri::command]
async fn cua_desktop_shot(max_dim: Option<i64>) -> Result<String, String> {
    blocking(move || cua::desktop_shot(max_dim.unwrap_or(900))).await
}
#[tauri::command]
async fn cua_display_count() -> Result<i64, String> {
    blocking(|| Ok(cua::display_count())).await
}
#[tauri::command]
fn cua_desktop_label() -> (&'static str, &'static str) {
    cua::desktop_label()
}
#[tauri::command]
async fn cua_get_state(pid: i64, window_id: i64, max_elements: Option<i64>) -> Result<Value, String> {
    blocking(move || cua::get_state(pid, window_id, max_elements.unwrap_or(200))).await
}
/// Only the user's explicit Activate control may move focus.
#[tauri::command]
async fn cua_activate(state: tauri::State<'_, RunManager>, pid: i64, window_id: i64, app_name: String) -> Result<Value, String> {
    let lease = state.begin_manual_input()?;
    blocking(move || { let _lease = lease; Ok(serde_json::json!({ "on_current_space": cua::ensure_current_space(pid, window_id, &app_name) })) }).await
}
#[tauri::command]
async fn cua_click(state: tauri::State<'_, RunManager>, pid: i64, window_id: i64, x: f64, y: f64, button: Option<String>, screenshot_id: Option<String>) -> Result<Value, String> {
    let lease = state.begin_manual_input()?;
    blocking(move || { let _lease = lease; cua::click(pid, window_id, x, y, button, screenshot_id.as_deref()) }).await
}
#[tauri::command]
async fn cua_type(state: tauri::State<'_, RunManager>, pid: i64, window_id: i64, text: String) -> Result<Value, String> {
    let lease = state.begin_manual_input()?;
    blocking(move || { let _lease = lease; cua::type_text(pid, window_id, text) }).await
}
#[tauri::command]
async fn cua_press_key(state: tauri::State<'_, RunManager>, pid: i64, window_id: i64, key: String) -> Result<Value, String> {
    let lease = state.begin_manual_input()?;
    blocking(move || { let _lease = lease; cua::press_key(pid, window_id, key) }).await
}
#[tauri::command]
async fn cua_scroll(state: tauri::State<'_, RunManager>, pid: i64, window_id: i64, x: f64, y: f64, dy: i64, amount: Option<i64>, screenshot_id: Option<String>) -> Result<Value, String> {
    let lease = state.begin_manual_input()?;
    blocking(move || { let _lease = lease; cua::scroll(pid, window_id, x, y, dy, amount.unwrap_or(3), screenshot_id.as_deref()) }).await
}
/// Read-only grant status for the selected app or external driver.
#[tauri::command]
async fn cua_permissions() -> Result<permissions::Status, String> {
    tauri::async_runtime::spawn_blocking(permissions::status).await
        .map_err(|error|format!("Could not check capture access: {error}"))
}
/// Explicit access/retry control. External driver retries remain passive.
#[tauri::command]
async fn cua_request_permissions() -> Result<permissions::Status, String> {
    tauri::async_runtime::spawn_blocking(permissions::request).await
        .map_err(|error|format!("Could not retry capture access: {error}"))
}
#[tauri::command]
fn cua_open_privacy_settings(pane: permissions::PrivacyPane) -> Result<(), String> {
    permissions::open_settings(pane)
}
/// Why an update of this install cannot be run by the signed-in user (Windows, all-users install, standard user).
#[tauri::command]
fn cua_update_blocked() -> Option<String> {
    permissions::updater_blocked()
}
/// True when this install's update is run by `elevated_update` (every installed kind on Windows: all users, per user, MSI).
#[tauri::command]
fn cua_update_owns_install() -> bool {
    elevated_update::owns_this_install()
}
/// How this install is updated (`all_users_nsis`, `per_user_nsis`, `msi`, `unknown`): the front end offers an MSI install
/// only an MSI.
#[tauri::command]
fn cua_update_install_kind() -> elevated_update::InstallKind {
    elevated_update::this_install_kind()
}
/// Download the pending update (the plugin checks its signature), then run its installer the way this kind of install
/// needs: all users elevated and waited for (a cancelled or refused prompt leaves this app and the installed copy as they
/// were), per user not elevated at all, MSI elevated and handed off (the app exits once the prompt is accepted; a cancelled
/// one keeps it). Progress goes to `on_event` in the shape the plugin's JS `downloadAndInstall` uses.
#[tauri::command]
async fn cua_install_update<R: tauri::Runtime>(app: tauri::AppHandle<R>, on_event: tauri::ipc::Channel<Value>)
    -> Result<elevated_update::Outcome, String> {
    use tauri_plugin_updater::UpdaterExt;
    let kind = elevated_update::this_install_kind();
    if !elevated_update::owns_install(kind) {
        return Err("this install is updated by the plugin, not by cua_install_update".into());
    }
    let update = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?
        .ok_or_else(|| "no update is available".to_string())?;
    let mut started = false;
    let progress = on_event.clone();
    let finished = on_event.clone();
    let bytes = update.download(
        move |chunk, total| {
            if !started {
                started = true;
                let _ = progress.send(serde_json::json!({"event": "Started", "data": {"contentLength": total}}));
            }
            let _ = progress.send(serde_json::json!({"event": "Progress", "data": {"chunkLength": chunk}}));
        },
        move || { let _ = finished.send(serde_json::json!({"event": "Finished"})); },
    ).await.map_err(|e| e.to_string())?;
    // An NSIS install runs an NSIS installer and an MSI install an MSI: a zip, or the other kind, is not what this path runs.
    if !elevated_update::package_matches(kind, &bytes) {
        return Err(format!("the update is not the installer this install ({kind:?}) is updated with"));
    }
    let safe: String = update.version.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-').collect();
    let dir = std::env::temp_dir().join(format!("copilot-update-{safe}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let msi = kind == elevated_update::InstallKind::Msi;
    let package = dir.join(format!("Copilot-{safe}-installer.{}", if msi { "msi" } else { "exe" }));
    std::fs::write(&package, &bytes).map_err(|e| e.to_string())?;
    let path = package.clone();
    let launch = tauri::async_runtime::spawn_blocking(move || match kind {
        elevated_update::InstallKind::AllUsersNsis => elevated_update::run_elevated(&path, elevated_update::installer_parameters()),
        elevated_update::InstallKind::PerUserNsis => elevated_update::run_unelevated(&path, elevated_update::per_user_installer_parameters()),
        _ => {
            // One elevated cmd: msiexec, then the user's own shell starts the app again at the user's integrity.
            let system = std::env::var("SYSTEMROOT").unwrap_or_else(|_| "C:\\Windows".into());
            let system32 = std::path::PathBuf::from(&system).join("System32");
            let msiexec = system32.join("msiexec.exe");
            let explorer = std::path::PathBuf::from(&system).join("explorer.exe");
            let app = std::env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
            match elevated_update::msi_elevated_command(&msiexec.to_string_lossy(), &path.to_string_lossy(), &app, &explorer.to_string_lossy()) {
                Some(line) => elevated_update::start_elevated(&system32.join("cmd.exe"), &line),
                None => elevated_update::Launch::NotStarted(elevated_update::ERROR_BAD_PATHNAME),
            }
        }
    }).await.map_err(|e| e.to_string())?;
    let outcome = elevated_update::classify(launch);
    match &outcome {
        elevated_update::Outcome::Kept { .. } => { let _ = std::fs::remove_dir_all(&dir); }
        // The MSI replaces the files of the running app: the app leaves, as the plugin's own install does, and the
        // installer starts it again (AUTOLAUNCHAPP).
        elevated_update::Outcome::HandedOff => {
            let handle = app.clone();
            std::thread::spawn(move || { std::thread::sleep(std::time::Duration::from_millis(400)); handle.exit(0); });
        }
        elevated_update::Outcome::Installed => {}
    }
    Ok(outcome)
}
/// The package format of a `.deb` or `.rpm` install (`deb` or `rpm`), which is updated by `cua_install_linux_package`; null otherwise.
#[tauri::command]
fn cua_update_linux_package() -> Option<&'static str> {
    linux_pkg::this_install_kind().map(|k| if k == linux_pkg::Kind::Rpm { "rpm" } else { "deb" })
}
/// Download the pending update of a `.deb` or `.rpm` install (the plugin checks its signature), refuse it before anything is
/// installed when the package needs something this machine lacks (the toast names it), then let the plugin install it
/// (`pkexec dpkg -i` or `pkexec rpm -U`). Progress goes to `on_event` in the shape the plugin's JS `downloadAndInstall` uses.
#[tauri::command]
async fn cua_install_linux_package<R: tauri::Runtime>(app: tauri::AppHandle<R>, on_event: tauri::ipc::Channel<Value>)
    -> Result<linux_pkg::Outcome, String> {
    use tauri_plugin_updater::UpdaterExt;
    let kind = linux_pkg::this_install_kind().ok_or_else(|| "this install is not a .deb or .rpm".to_string())?;
    let update = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?
        .ok_or_else(|| "no update is available".to_string())?;
    let mut started = false;
    let progress = on_event.clone();
    let finished = on_event.clone();
    let bytes = update.download(
        move |chunk, total| {
            if !started {
                started = true;
                let _ = progress.send(serde_json::json!({"event": "Started", "data": {"contentLength": total}}));
            }
            let _ = progress.send(serde_json::json!({"event": "Progress", "data": {"chunkLength": chunk}}));
        },
        move || { let _ = finished.send(serde_json::json!({"event": "Finished"})); },
    ).await.map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        linux_pkg::install_checked(kind, &bytes, &linux_pkg::System, |b| update.install(b).map_err(|e| e.to_string()))
    }).await.map_err(|e| e.to_string())?
}
#[tauri::command]
async fn cua_list_apps()-> Result<Value, String> {
    blocking(cua::list_apps).await
}
#[tauri::command]
async fn cua_launch_app(app: String) -> Result<Value, String> {
    blocking(move || cua::launch_app(&app)).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = configure_commands(tauri::Builder::default());
    // The single-instance plugin must be the first plugin registered: a second launch (for
    // example clicking an icu:// invite link while the app is already open) forwards its argv to
    // the running instance instead of starting a second copy. Its `deep-link` feature means the
    // forwarded icu:// URL reaches the deep-link plugin's on_open_url handler below.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
        if let Some(window) = app.get_webview_window("main") { let _ = window.set_focus(); }
    }));
    builder
        // icu:// is the invite deep link (icu://enroll?invite=...&h=...). The scheme is declared
        // for bundling in tauri.conf.json (macOS Info.plist / Windows registry) and claimed at
        // runtime in setup() for dev builds and Linux.
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        // Auto-update: checks the configured endpoint on startup; the frontend
        // surfaces "update available" and installs on confirm. Desktop only.
        .plugin({
            let mut updater = tauri_plugin_updater::Builder::new();
            // Windows: the update installer keeps the scope (all users or current user) of the install it replaces.
            if let Some(arg) = permissions::updater_installer_arg() { updater = updater.installer_arg(arg); }
            updater.build()
        })
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            // Load any onboarded API key into the process-global cache so a
            // Finder-launched app (no ANTHROPIC_API_KEY in its env) still has a model.
            onboarding::load_into_cache(app.handle());
            memory::start_worker(app.handle().clone(), std::time::Duration::from_secs(30));
            let history_app=app.handle().clone();
            app.state::<history::Service>().start(move|context,timestamp,payload|history_app.state::<memory::MemoryService>().record_history_context(context,timestamp,payload));
            fit_main_window(app);
            // Ingest icu://enroll invite links. Redeem and the device token stay in Rust
            // (fleet.rs); the webview only ever sees a refreshed fleet status.
            #[cfg(desktop)]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                let _ = app.deep_link().register_all();
                let enroll_app = app.handle().clone();
                app.deep_link().on_open_url(move |event| {
                    for url in event.urls() { fleet::ingest_enroll_url(&enroll_app, url.as_str()); }
                });
            }
            Ok(())
        })
        // Serve window/desktop frames as raw image bytes over a custom protocol,
        // captured on a worker thread — so the webview never decodes a giant
        // base64 data URL on its main thread (the pinwheel source), and the
        // capture never blocks the UI.
        .register_asynchronous_uri_scheme_protocol("frame", |_app, request, responder| {
            let uri = request.uri().clone();
            let if_none_match = request
                .headers()
                .get("if-none-match")
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);
            let Some(lease) = FrameLease::acquire(uri.path()) else {
                responder.respond(empty_response(429));
                return;
            };
            std::thread::spawn(move || {
                let _lease = lease;
                let path = uri.path().trim_start_matches('/').to_string();
                let parts: Vec<&str> = path.split('/').collect();
                // Canonical per-surface cache key, stable across the ?t= buster.
                // A window frame carries the driver's id for that capture, so a
                // click on the tile can name the frame it was read from.
                let keyed: Option<(String, Box<frame_serve::Capture>)> = match parts.as_slice() {
                    ["win", pid, wid, max, ..] => {
                        let (pid, wid, max) =
                            (pid.parse().unwrap_or(0), wid.parse().unwrap_or(0), max.parse().unwrap_or(640));
                        Some((
                            format!("win/{pid}/{wid}/{max}"),
                            Box::new(move || cua::window_frame(pid, wid, max)),
                        ))
                    }
                    ["desktop", max, ..] => {
                        let max = max.parse().unwrap_or(900);
                        Some((
                            format!("desktop/{max}"),
                            Box::new(move || cua::desktop_shot_bytes(max).map(|b| (b, "image/png".into(), None))),
                        ))
                    }
                    ["display", n, max, ..] => {
                        let (n, max) = (n.parse().unwrap_or(1), max.parse().unwrap_or(1100));
                        Some((
                            format!("display/{n}/{max}"),
                            Box::new(move || cua::display_shot_bytes(n, max).map(|b| (b, "image/png".into(), None))),
                        ))
                    }
                    _ => None,
                };
                let resp = match keyed {
                    None => empty_response(204),
                    Some((key, capture)) => {
                        match frame_serve::serve(&key, if_none_match.as_deref(), capture) {
                            frame_serve::Served::Modified(frame) => image_response(frame),
                            frame_serve::Served::NotModified { etag } => not_modified_response(etag),
                            // The driver's typed reason for this window, shown on the tile.
                            frame_serve::Served::Refused(reason) => refusal_response(reason),
                            frame_serve::Served::Empty => empty_response(204),
                        }
                    }
                };
                responder.respond(resp);
            });
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) { close_for_exit(app); }
        });
}

fn refusal_response(reason: String) -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder().status(422)
        .header("Content-Type", "text/plain; charset=utf-8")
        .header("Cache-Control", "no-store")
        .header("Access-Control-Allow-Origin", "*")
        .body(reason.into_bytes()).unwrap()
}

/// A live frame body. JPEG for the feed (a fraction of the PNG cost), with an
/// ETag so the next GET of an unchanged surface can be answered 304, and the
/// driver's screenshot id exposed cross-origin for pixel binding.
fn image_response(frame: frame_serve::Frame) -> tauri::http::Response<Vec<u8>> {
    let mut response = tauri::http::Response::builder().status(200)
        .header("Content-Type", frame.mime)
        .header("ETag", &frame.etag)
        // Let the webview revalidate with If-None-Match rather than refetch blind.
        .header("Cache-Control", "no-cache")
        .header("Access-Control-Allow-Origin", "*")
        .header("Access-Control-Expose-Headers", "X-Screenshot-Id, ETag");
    if let Some(id) = frame.screenshot_id {
        response = response.header("X-Screenshot-Id", id);
    }
    response.body(frame.bytes).unwrap()
}

/// The surface's pixels are unchanged since the caller's ETag: no body.
fn not_modified_response(etag: String) -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder().status(304)
        .header("ETag", etag)
        .header("Cache-Control", "no-cache")
        .header("Access-Control-Allow-Origin", "*")
        .header("Access-Control-Expose-Headers", "X-Screenshot-Id, ETag")
        .body(Vec::new()).unwrap()
}

/// A bodyless status (204 no frame, 429 too many captures in flight).
fn empty_response(status: u16) -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder().status(status)
        .header("Cache-Control", "no-store")
        .header("Access-Control-Allow-Origin", "*")
        .body(Vec::new()).unwrap()
}

// A timed-out image request must not accumulate capture threads forever. At
// most one capture per surface, and four total, may be in flight.
struct FrameLease(String);
static FRAMES: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<String>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));
impl FrameLease {
    fn acquire(path: &str) -> Option<Self> {
        let mut frames = FRAMES.lock().unwrap();
        if frames.len() >= 4 || !frames.insert(path.into()) { return None; }
        Some(Self(path.into()))
    }
}
impl Drop for FrameLease {
    fn drop(&mut self) { FRAMES.lock().unwrap().remove(&self.0); }
}

/// Whether a usable model is configured, and from which sources, so the UI can
/// show onboarding to a fresh user. Reads the environment, the stored onboarded
/// key, and whether the `claude` CLI is installed.
#[tauri::command]
fn model_onboarding_status() -> Value {
    let env_set = |k: &str| std::env::var(k).ok().map(|v| !v.trim().is_empty()).unwrap_or(false);
    onboarding::status(
        env_set("ANTHROPIC_API_KEY"),
        env_set("OPENAI_API_KEY") || env_set("ICU_OPENAI_API_KEY"),
        brain::claude_available(),
    )
}

/// Store (or, when the key is blank, clear) a provider's API key in the app's own
/// data dir and start using it. Returns the refreshed onboarding status.
#[tauri::command]
fn set_model_key<R: tauri::Runtime>(app: tauri::AppHandle<R>, provider: String, key: String) -> Result<Value, String> {
    onboarding::set_key(&app, &provider, &key)?;
    Ok(model_onboarding_status())
}

fn configure_commands<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    configure_commands_with(builder, std::sync::Arc::new(agent::run::<R>))
}

fn configure_commands_with<R: tauri::Runtime>(builder: tauri::Builder<R>, runner: TaskBody<R>) -> tauri::Builder<R> {
    let memories=memory::MemoryService::open(memory::default_root());
    let history=history::Service::open(memory::default_root().join("computer-history"),memories.settings().skysight,memories.settings().paused);
    builder.manage(RunManager::default()).manage(TaskRunner(runner)).manage(run_control::Confirmations::default())
        .manage(audio::Service::default())
        .manage(history).manage(memories).manage(memory::workflows::Runtime::default())
        .manage(fleet::Service::default())
        .invoke_handler(tauri::generate_handler![
            computer_history_status,
            computer_history_configure,
            computer_history_setup_desktop,
            computer_history_pause,
            computer_history_settings_set,
            computer_history_clear,
            computer_history_delete_preview,
            computer_history_delete_reviewed,
            history_summary_status,
            history_summary_configure,
            history_summary_list,
            history_summary_reveal,
            computer_history_retention_set,
            computer_history_capture_mode,
            computer_history_browser_capture,
            computer_history_browser_permission,
            agent_run,
            agent_stop,
            agent_confirm,
            agent_status,
            agent_checkpoints,
            agent_checkpoint_discard,
            cua_status,
            cua_list_windows,
            cua_window_shot,
            cua_desktop_shot,
            cua_display_count,
            cua_desktop_label,
            cua_get_state,
            cua_activate,
            cua_click,
            cua_type,
            cua_press_key,
            cua_scroll,
            cua_list_apps,
            cua_launch_app,
            cua_permissions,
            cua_update_blocked,
            cua_update_owns_install,
            cua_update_install_kind,
            cua_install_update,
            cua_update_linux_package,
            cua_install_linux_package,
            cua_request_permissions,
            cua_open_privacy_settings,
            memory_status,
            memory_settings_set,
            memory_pause,
            memory_list,
            memory_source,
            memory_delete,
            trajectory_preview,
            trajectory_preview_page,
            trajectory_import,
            workflow_generate,
            workflow_cancel,
            workflow_save,
            workflow_list,
            workflow_delete,
            fleet::fleet_status,
            fleet::fleet_set_exec_optin,
            fleet::fleet_decline,
            fleet::fleet_unlink,
            fleet::fleet_stage,
            fleet::fleet_enroll,
            audio_start,
            audio_stop,
            audio_status,
            audio_export_wav,
            model_onboarding_status,
            set_model_key,
        ])
}

#[cfg(test)]
mod window_fit_tests {
    use super::fitted_size;
    #[test]
    fn the_window_fits_the_work_area_and_keeps_its_size_on_big_screens() {
        let want = (1280.0, 820.0);
        let min = (980.0, 640.0);
        // 1280x800 @2x with the menu bar and Dock: about 1280x706 usable.
        let ((w, h), _) = fitted_size((1280.0, 706.0), want, min);
        assert!(w <= 1280.0 * 0.94 && h <= 706.0 * 0.92, "{w}x{h} covers the screen");
        assert_eq!(fitted_size((1920.0, 1150.0), want, min).0, want, "a big screen keeps the configured size");
        let ((w, h), (mw, mh)) = fitted_size((900.0, 560.0), want, min);
        assert!(w <= 900.0 && h <= 560.0 && mw <= w && mh <= h, "the minimum gives way on a tiny screen");
    }
    #[test]
    fn the_shrunk_window_is_centred_in_the_work_area() {
        // 1280x800 @2x: the work area starts below the 31 pt menu bar and is 1280x706.
        let (x, y) = super::centered((0.0, 31.0), (1280.0, 706.0), (1204.0, 650.0));
        assert_eq!((x, y), (38.0, 59.0));
    }
}

#[cfg(test)]
mod ipc_tests {
    #[test]
    fn status_and_stop_use_the_shared_backend_task() {
        use tauri::Manager;
        let app = super::configure_commands(tauri::test::mock_builder())
            .build(tauri::test::mock_context(tauri::test::noop_assets())).unwrap();
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
        let request = |cmd: &str, body| tauri::webview::InvokeRequest {
            cmd: cmd.into(), callback: tauri::ipc::CallbackFn(0), error: tauri::ipc::CallbackFn(1),
            url: if cfg!(windows) { "http://tauri.localhost" } else { "tauri://localhost" }.parse().unwrap(),
            body: tauri::ipc::InvokeBody::Json(body), headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.into(),
        };
        // An admitted fixture task, with no worker and no desktop connection.
        let task = app.state::<super::RunManager>().begin(super::RunInfo {
            id: "fixture-run".into(), goal: "fixture".into(), pid: 42, window_id: 7, app_name: "fixture".into(),
        }).unwrap();
        let status = tauri::test::get_ipc_response(&webview, request("agent_status", serde_json::json!({})))
            .unwrap().deserialize::<serde_json::Value>().unwrap();
        assert_eq!(status["runId"], "fixture-run");
        assert_eq!(status["stopping"], false);
        tauri::test::assert_ipc_response(&webview, request("agent_stop", serde_json::json!({"runId":"older-run"})), Ok::<_, bool>(false));
        assert!(!task.cancelled());
        tauri::test::assert_ipc_response(&webview, request("agent_stop", serde_json::json!({"runId":"fixture-run"})), Ok::<_, bool>(true));
        assert!(task.cancelled());
        assert!(app.state::<super::RunManager>().status().unwrap().is_some());
        drop(task);
        tauri::test::assert_ipc_response(&webview, request("agent_status", serde_json::json!({})), Ok::<_, serde_json::Value>(serde_json::Value::Null));
    }

    #[test]
    fn a_confirmation_answer_reaches_only_the_running_tasks_open_request() {
        use tauri::Manager;
        let app = super::configure_commands(tauri::test::mock_builder())
            .build(tauri::test::mock_context(tauri::test::noop_assets())).unwrap();
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
        let request = |run: &str, req: &str, approve: bool| tauri::webview::InvokeRequest {
            cmd: "agent_confirm".into(), callback: tauri::ipc::CallbackFn(0), error: tauri::ipc::CallbackFn(1),
            url: if cfg!(windows) { "http://tauri.localhost" } else { "tauri://localhost" }.parse().unwrap(),
            body: tauri::ipc::InvokeBody::Json(serde_json::json!({"runId": run, "requestId": req, "approve": approve})),
            headers: Default::default(), invoke_key: tauri::test::INVOKE_KEY.into(),
        };
        let task = app.state::<super::RunManager>().begin(super::RunInfo {
            id: "run-a".into(), goal: "fixture".into(), pid: 42, window_id: 7, app_name: "fixture".into(),
        }).unwrap();
        let boxes = app.state::<super::run_control::Confirmations>();
        let cancelled = || false;
        let asker = super::run_control::Asker { boxes: boxes.inner(), run_id: "run-a", cancelled: &cancelled,
            timeout: std::time::Duration::from_secs(10) };
        use crate::gate::Confirm;
        asker.open("c1");
        // Another run, an unopened request, or a key with a separator: not answered.
        for (run, req) in [("run-b", "c1"), ("run-a", "c2"), ("run-a", "c1/x")] {
            tauri::test::assert_ipc_response(&webview, request(run, req, true), Ok::<_, bool>(false));
        }
        tauri::test::assert_ipc_response(&webview, request("run-a", "c1", false), Ok::<_, bool>(true));
        assert_eq!(asker.wait("c1"), crate::gate::Consent::Declined);
        tauri::test::assert_ipc_response(&webview, request("run-a", "c1", true), Ok::<_, bool>(false));
        // Once the task has ended, nothing is answered.
        asker.open("c3");
        drop(task);
        tauri::test::assert_ipc_response(&webview, request("run-a", "c3", true), Ok::<_, bool>(false));
    }

    #[test]
    fn allow_after_stop_is_refused_and_a_reload_gets_the_open_question() {
        use tauri::Manager;
        let app = super::configure_commands(tauri::test::mock_builder())
            .build(tauri::test::mock_context(tauri::test::noop_assets())).unwrap();
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
        let call = |cmd: &str, body: serde_json::Value| tauri::webview::InvokeRequest {
            cmd: cmd.into(), callback: tauri::ipc::CallbackFn(0), error: tauri::ipc::CallbackFn(1),
            url: if cfg!(windows) { "http://tauri.localhost" } else { "tauri://localhost" }.parse().unwrap(),
            body: tauri::ipc::InvokeBody::Json(body), headers: Default::default(), invoke_key: tauri::test::INVOKE_KEY.into(),
        };
        let task = app.state::<super::RunManager>().begin(super::RunInfo {
            id: "run-s".into(), goal: "fixture".into(), pid: 42, window_id: 7, app_name: "fixture".into(),
        }).unwrap();
        let boxes = app.state::<super::run_control::Confirmations>();
        // No question open: the status carries none.
        let status = tauri::test::get_ipc_response(&webview, call("agent_status", serde_json::json!({}))).unwrap().deserialize::<serde_json::Value>().unwrap();
        assert!(status["pendingConfirm"].is_null());
        // A question is open: a reloaded window reads it from the status.
        boxes.open("run-s/c1");
        let ask = serde_json::json!({"id": "c1", "action": "click \"Send\"", "why": "its target \"Send\" sends", "app": "Mail"});
        boxes.describe("run-s/c1", ask.clone());
        let status = tauri::test::get_ipc_response(&webview, call("agent_status", serde_json::json!({}))).unwrap().deserialize::<serde_json::Value>().unwrap();
        assert_eq!(status["pendingConfirm"], ask);
        // After Stop, an Allow is refused and the status stops offering the question.
        assert!(app.state::<super::RunManager>().stop("run-s").unwrap());
        tauri::test::assert_ipc_response(&webview, call("agent_confirm", serde_json::json!({"runId": "run-s", "requestId": "c1", "approve": true})), Ok::<_, bool>(false));
        let status = tauri::test::get_ipc_response(&webview, call("agent_status", serde_json::json!({}))).unwrap().deserialize::<serde_json::Value>().unwrap();
        assert!(status["pendingConfirm"].is_null());
        // The refused Allow recorded nothing, so the waiter still sees the stop.
        assert_eq!(boxes.wait("run-s/c1", std::time::Duration::from_secs(1), &|| task.cancelled()), crate::gate::Consent::Stopped);
    }

    #[test]
    fn capture_requests_are_bounded_and_released() {
        let first = super::FrameLease::acquire("/win/test").unwrap();
        assert!(super::FrameLease::acquire("/win/test").is_none());
        drop(first);
        assert!(super::FrameLease::acquire("/win/test").is_some());
    }
    #[test]
    fn chat_command_reaches_the_real_backend_handler() {
        // MockRuntime creates no native window and sends no desktop input. The
        // handler registry is the same one the packaged app uses.
        let app = super::configure_commands(tauri::test::mock_builder())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build().unwrap();
        tauri::test::assert_ipc_response(&webview, tauri::webview::InvokeRequest {
            cmd: "agent_run".into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: if cfg!(windows) { "http://tauri.localhost" } else { "tauri://localhost" }.parse().unwrap(),
            body: tauri::ipc::InvokeBody::Json(serde_json::json!({
                "goal": "", "pid": 42, "windowId": 7, "appName": "Fixture", "brain": "local"
            })),
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.into(),
        }, Err::<&str, _>("goal must not be empty"));
    }
}

#[cfg(test)]
mod frame_protocol_tests {
    use super::{image_response, not_modified_response};
    use crate::frame_serve::Frame;

    fn frame(id: Option<&str>) -> Frame {
        Frame { bytes: vec![1], mime: "image/jpeg", etag: "\"f0000abcd-4\"".into(), screenshot_id: id.map(str::to_string) }
    }

    #[test]
    fn a_window_frame_exposes_its_screenshot_id_and_etag_to_the_webview() {
        let response = image_response(frame(Some("f0000abcd-4")));
        assert_eq!(response.headers()["content-type"], "image/jpeg");
        assert_eq!(response.headers()["x-screenshot-id"], "f0000abcd-4");
        assert_eq!(response.headers()["etag"], "\"f0000abcd-4\"");
        // The webview reads these headers cross-origin (frame:// vs the app origin).
        assert_eq!(response.headers()["access-control-expose-headers"], "X-Screenshot-Id, ETag");
        assert!(image_response(frame(None)).headers().get("x-screenshot-id").is_none());
    }

    #[test]
    fn a_not_modified_response_carries_the_etag_and_no_body() {
        let response = not_modified_response("\"f0000abcd-4\"".into());
        assert_eq!(response.status(), 304);
        assert_eq!(response.headers()["etag"], "\"f0000abcd-4\"");
        assert!(response.body().is_empty());
    }
}
