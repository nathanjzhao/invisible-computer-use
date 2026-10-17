//! Public observation calls over a local synthetic daemon. No desktop APIs or input.
use super::*;
use std::os::unix::fs::PermissionsExt;

fn observation() -> Value {
    json!({"pid":42,"window_id":7,"snapshot_id":"snapshot-current",
        "window_bounds":{"width":64,"height":48},"screenshot_width":32,"screenshot_height":24,
        "screenshot_frame_valid":true,"elements_complete":false,
        "elements":[{"element_index":0,"role":"AXTextField","label":"Editor","element_token":"current-token"}],
        "passive_elements":[{"role":"AXStaticText","label":"Status"}],
        "ax_diagnostics":{"observer_trusted":true}})
}

fn fixture(case: &str) -> (CaptureFile, String, String) {
    warm_python_shim();
    let dir = CaptureFile::new().unwrap();
    let binary = dir.0.join("observation-driver");
    std::fs::write(&binary, include_str!("../../tests/fixtures/fake_lifetime_driver.py")).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    use cua_driver_contract::{CONTRACT_VERSION, TOOLS_LIST_SCHEMA_VERSION, CAPABILITY_VERSION, MCP_PROTOCOL_VERSION};
    std::fs::write(dir.0.join("metadata.json"), json!({"driver_version":"fixture",
        "contract_version": if case == "incompatible" { "incompatible" } else { CONTRACT_VERSION },
        "tools_list_schema_version":TOOLS_LIST_SCHEMA_VERSION,"capability_version":CAPABILITY_VERSION,
        "mcp_protocol_version":MCP_PROTOCOL_VERSION}).to_string()).unwrap();
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(32,24).write_to(&mut bytes,image::ImageFormat::Png).unwrap();
    let bytes=bytes.into_inner();
    std::fs::write(dir.0.join("image.png"),&bytes).unwrap();
    std::fs::write(dir.0.join("observation.json"),observation().to_string()).unwrap();
    if matches!(case,"refused"|"structured-error") {
        use base64::Engine;
        let mut fields=observation();
        fields["code"]=json!("permission_denied"); fields["message"]=json!("fixture observation refused");
        if case=="structured-error" { fields["isError"]=json!(true); }
        std::fs::write(dir.0.join("capture-refusal.json"),json!({"isError":case=="refused",
            "content":[{"type":"text","text":"fixture observation refused"},
                {"type":"image","mimeType":"image/png","data":base64::engine::general_purpose::STANDARD.encode(bytes)}],
            "structuredContent":fields}).to_string()).unwrap();
    }
    if matches!(case,"capture-denied"|"external-capture-denied") {
        std::fs::write(dir.0.join("capture-refusal.json"),json!({"isError":true,
            "content":[{"type":"text","text":"capture denied by system"}],
            "structuredContent":{"screenshot_frame_valid":false,"screenshot_error":{
                "code":"capture_permission_denied","permission":"screen_capture",
                "error_domain":"com.apple.ScreenCaptureKit.SCStreamErrorDomain","error_code":-3801}}}).to_string()).unwrap();
    }
    if matches!(case,"action-denied-direct"|"action-denied-cli") {
        std::fs::write(dir.0.join("click-error.json"),json!({"isError":true,
            "content":[{"type":"text","text":"native capture validation was denied"}],
            "structuredContent":{"code":"capture_permission_denied","window_id":7,"permission":"screen_capture",
                "error_domain":"com.apple.ScreenCaptureKit.SCStreamErrorDomain","error_code":-3801,
                "reason":"fixture native capture validation denial"}}).to_string()).unwrap();
    }
    let socket = dir.0.join("o").to_string_lossy().into_owned();
    (dir,binary.to_string_lossy().into_owned(),socket)
}

fn child(test: &str, case: &str) -> Vec<Value> {
    let (dir,binary,socket) = fixture(case);
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args([&format!("cua::observation_tests::{test}"),"--exact","--nocapture"])
        .env("ICU_OBSERVATION_CHILD",case).env("ICU_CUA_BIN",binary)
        .env("ICU_TEST_OWN_SOCKET",&socket).env("ICU_OWN_DRIVER","1").env_remove("ICU_CUA_SOCKET");
    if case == "unavailable" { command.env("ICU_CUA_SOCKET",&socket); }
    match case {
        "ax-only" => {command.env("ICU_TEST_IO_PERMISSIONS","ax_only");},
        "denied-grants" => {command.env("ICU_TEST_IO_PERMISSIONS","none");},
        "external-denied-host" | "external-status" | "external-capture-denied" | "action-denied-direct" | "action-denied-cli" => {command.env("ICU_CUA_SOCKET",&socket).env("ICU_TEST_IO_PERMISSIONS","none");},
        _ => {},
    }
    let output = crate::child_test_output(&mut command,Duration::from_secs(20)).unwrap();
    let rows: Vec<Value> = std::fs::read_to_string(dir.0.join("requests.jsonl")).unwrap_or_default()
        .lines().map(|line|serde_json::from_str(line).unwrap()).collect();
    assert!(output.status.success(),"{case}: {}\n{}\n{rows:?}",String::from_utf8_lossy(&output.stdout),String::from_utf8_lossy(&output.stderr));
    rows
}

fn driver_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("ICU_CUA_BIN").unwrap()).parent().unwrap().to_owned()
}

#[test]
fn missing_owned_grants_refuse_before_driver_start_or_capture_fallback() {
    if std::env::var_os("ICU_OBSERVATION_CHILD").is_none() {
        let rows=child("missing_owned_grants_refuse_before_driver_start_or_capture_fallback","denied-grants");
        assert!(rows.is_empty(),"missing grants sent driver requests: {rows:?}");
        return;
    }
    assert!(window_frame(42,7,640).unwrap_err().starts_with(CAPTURE_REFUSED));
    assert!(get_state(42,7,200).unwrap_err().contains("Accessibility"));
    assert!(call("set_value",&json!({"pid":42,"window_id":7,"value":"blocked"})).unwrap_err().contains("Accessibility"));
    assert!(input("set_value",json!({"pid":42,"window_id":7,"value":"blocked"})).unwrap_err().message().contains("Accessibility"));
    assert!(desktop_shot_bytes(100).unwrap_err().contains("Screen Recording"));
    assert!(!std::path::Path::new(&std::env::var("ICU_TEST_OWN_SOCKET").unwrap()).exists());
}

#[test]
fn missing_screen_permission_keeps_ax_reads_without_forcing_capture() {
    if std::env::var_os("ICU_OBSERVATION_CHILD").is_none() {
        let rows=child("missing_screen_permission_keeps_ax_reads_without_forcing_capture","ax-only");
        let reads:Vec<_>=rows.iter().filter(|r|r["name"]=="get_window_state").collect();
        assert_eq!(reads.len(),2);
        assert!(reads.iter().all(|r|r["args"]["include_screenshot"]==false&&r["args"].get("screenshot_out_file").is_none()));
        return;
    }
    assert!(input("click",json!({"pid":42,"window_id":7,"element_token":"semantic-selection"})).unwrap_err().message().contains("Screen Recording"));
    assert!(input("scroll",json!({"pid":42,"window_id":7,"x":1,"y":2})).unwrap_err().message().contains("Screen Recording"));
    let state=get_state(42,7,200).unwrap();
    assert!(state["elements"].as_array().is_some_and(|els|!els.is_empty()));
    assert!(state.get("screenshot_data_url").is_none());
    assert_eq!(state["screenshot_error"]["code"],"screen_recording_permission_required");
    assert!(ui_readiness::unusable_reason(&state).is_none(),"AX remains useful");
    // Exercise the compatibility helper too: it must not add the file option
    // that the native driver treats as an explicit capture request.
    cli_observation(json!({"pid":42,"window_id":7,"include_accessibility_tree":true,"include_screenshot":false})).unwrap();
    shutdown_owned_driver();
}

#[test]
fn external_driver_is_independent_but_host_display_capture_still_checks_host_grants() {
    if std::env::var_os("ICU_OBSERVATION_CHILD").is_none() {
        let rows=child("external_driver_is_independent_but_host_display_capture_still_checks_host_grants","external-denied-host");
        let reads:Vec<_>=rows.iter().filter(|r|r["name"]=="get_window_state").collect();
        assert_eq!(reads.len(),1);assert_eq!(reads[0]["args"]["include_screenshot"],true);
        return;
    }
    let state=get_state(42,7,200).unwrap();
    assert!(state["screenshot_data_url"].is_string(),"external driver owns its grant");
    #[cfg(target_os="macos")]
    assert!(display_shot_bytes(1,100).unwrap_err().contains("Screen Recording"),"host subprocess must not run");
}

#[test]
fn external_permission_checks_and_request_never_probe_or_prompt_the_host() {
    if std::env::var_os("ICU_OBSERVATION_CHILD").is_none() {
        let rows=child("external_permission_checks_and_request_never_probe_or_prompt_the_host","external-status");
        assert_eq!(rows.len(),6,"only passive probes: {rows:?}");
        assert!(rows.iter().all(|row| matches!(row["name"].as_str(),Some("check_permissions"|"health_report"))));
        for row in rows.iter().filter(|row|row["name"]=="check_permissions") {
            assert_eq!(row["args"],json!({"prompt":false,"probe_direct_capture":false}));
            assert!(row["session"].is_null(),"no cursor/session initialization");
        }
        return;
    }
    let mut fixture=DriverProcess::default();
    assert!(fixture.ensure(&cua_bin(),&socket_path().unwrap()));
    // Host grants are denied by the child override. The endpoint's grants win.
    let initial=crate::permissions::status();
    assert_eq!((initial.source,initial.accessibility,initial.screen_recording),("driver",true,true));
    assert!(initial.error.is_none());
    assert_eq!(crate::permissions::request(),initial,"external request stays passive");
    assert!(crate::permissions::open_settings(crate::permissions::PrivacyPane::ScreenRecording).is_err());
    std::fs::write(driver_dir().join("permissions.json"),json!({"isError":true}).to_string()).unwrap();
    std::fs::write(driver_dir().join("health.json"),json!({"platform":"darwin","checks":[
        {"name":"tcc_accessibility","status":"pass"},{"name":"tcc_screen_recording","status":"fail"}]}).to_string()).unwrap();
    let fallback=crate::permissions::status();
    assert_eq!((fallback.source,fallback.accessibility,fallback.screen_recording),("driver",true,false));
    assert!(fallback.error.is_none());
    std::fs::write(driver_dir().join("health.json"),json!({"platform":"darwin","checks":[]}).to_string()).unwrap();
    let unavailable=crate::permissions::status();
    assert_eq!(unavailable.source,"driver");
    assert!(unavailable.error.is_some(),"unknown external status must not report host grants");
}

#[test]
fn definite_capture_denial_pauses_queued_previews_but_keeps_ax_until_explicit_retry() {
    if std::env::var_os("ICU_OBSERVATION_CHILD").is_none() {
        let rows=child("definite_capture_denial_pauses_queued_previews_but_keeps_ax_until_explicit_retry","capture-denied");
        let captures:Vec<_>=rows.iter().filter(|r|r["name"]=="get_window_state"&&r["args"]["include_screenshot"]==true).collect();
        assert_eq!(captures.len(),2,"one denial and one deliberate retry, no automatic repeat: {rows:?}");
        assert!(rows.iter().any(|r|r["name"]=="get_window_state"&&r["args"]["include_screenshot"]==false));
        assert!(captures.iter().all(|r|r["transport"].is_null()),"denial must never trigger a CLI capture retry");
        return;
    }
    std::thread::scope(|scope| {
        let attempts:Vec<_>=(0..6).map(|_|scope.spawn(||window_frame(42,7,640))).collect();
        for attempt in attempts {assert!(attempt.join().unwrap().unwrap_err().contains("capture_permission_denied"));}
    });
    for _ in 0..2 {assert!(crate::permissions::status().error.is_some());}
    #[cfg(target_os="macos")]
    assert!(display_shot_bytes(1,100).unwrap_err().contains("capture_permission_denied"),"host display subprocess must also honor the app denial");
    daemon_reset_session();
    assert!(window_frame(42,7,640).unwrap_err().contains("capture_permission_denied"),"session reset is not consent");
    let state=get_state(42,7,200).unwrap();
    assert_eq!(state["screenshot_error"]["code"],"capture_permission_denied");
    assert!(ui_readiness::unusable_reason(&state).is_none());
    assert!(state.get("screenshot_data_url").is_none());
    retry_capture_permission_after_user_action(false);
    assert!(capture_permission_error().is_some(),"failed preflight cannot clear denial");
    // Exercise the production reset helper without requesting local OS grants.
    retry_capture_permission_after_user_action(true);
    assert!(capture_permission_error().is_none());
    std::fs::remove_file(driver_dir().join("capture-refusal.json")).unwrap();
    assert!(window_frame(42,7,640).is_ok());
    shutdown_owned_driver();
}

#[test]
fn external_capture_denial_retries_only_its_endpoint_without_host_grants() {
    if std::env::var_os("ICU_OBSERVATION_CHILD").is_none() {
        let rows=child("external_capture_denial_retries_only_its_endpoint_without_host_grants","external-capture-denied");
        let captures:Vec<_>=rows.iter().filter(|r|r["name"]=="get_window_state"&&r["args"]["include_screenshot"]==true).collect();
        assert_eq!(captures.len(),2,"automatic external capture repeated: {rows:?}");
        assert!(rows.iter().filter(|r|r["name"]=="check_permissions").all(|r|r["args"]==json!({"prompt":false,"probe_direct_capture":false})));
        return;
    }
    let mut fixture=DriverProcess::default();
    assert!(fixture.ensure(&cua_bin(),&socket_path().unwrap()));
    assert!(window_frame(42,7,640).unwrap_err().contains("capture_permission_denied"));
    assert!(window_frame(42,7,640).unwrap_err().contains("capture_permission_denied"));
    assert!(capture_denial_for("app").is_none(),"driver denial cannot become host denial");
    let status=crate::permissions::status();
    assert_eq!(status.source,"driver");assert!(status.error.is_some());
    assert!(status.screen_recording,"current driver preflight alone cannot clear an explicit denial");
    let state=get_state(42,7,200).unwrap();
    assert_eq!(state["screenshot_error"]["code"],"capture_permission_denied");
    assert!(state.get("screenshot_data_url").is_none());
    let selected=std::env::var("ICU_CUA_SOCKET").unwrap();
    std::env::set_var("ICU_CUA_SOCKET",format!("{selected}.other"));
    assert!(capture_permission_error().is_none(),"another endpoint must not inherit denial");
    std::env::set_var("ICU_CUA_SOCKET",&selected);
    assert!(capture_permission_error().is_some(),"returning to denied endpoint retains pause");
    std::fs::write(driver_dir().join("permissions.json"),json!({"accessibility":true,"screen_recording":false}).to_string()).unwrap();
    assert!(crate::permissions::request().error.is_some(),"a denied passive preflight cannot authorize retry");
    std::fs::write(driver_dir().join("permissions.json"),json!({"accessibility":true,"screen_recording":true}).to_string()).unwrap();
    assert!(crate::permissions::request().error.is_none(),"explicit retry uses external preflight, never host grants");
    std::fs::remove_file(driver_dir().join("capture-refusal.json")).unwrap();
    assert!(window_frame(42,7,640).is_ok());
}

fn action_capture_denial_scenario(cli_fallback: bool) {
    // Both cases use an external synthetic endpoint. The host is denied in the
    // child environment, so the production Retry access command stays passive
    // and cannot request native permissions on the developer computer.
    let mut fixture=DriverProcess::default();
    if !cli_fallback { assert!(fixture.ensure(&cua_bin(),&socket_path().unwrap())); }
    else { assert!(!std::path::Path::new(&socket_path().unwrap()).exists()); }

    let error=click_element(42,7,"current-token",None,None,None).unwrap_err();
    assert_eq!(error.code(),Some("capture_permission_denied"));
    let ActionFailure::Driver(error)=error else {panic!("native action denial lost its typed payload")};
    let structured=error.structured.unwrap();
    assert_eq!(structured["error_domain"],"com.apple.ScreenCaptureKit.SCStreamErrorDomain");
    assert_eq!(structured["error_code"],-3801);
    assert_eq!(structured["permission"],"screen_capture");

    let after_action=std::fs::read_to_string(driver_dir().join("requests.jsonl")).unwrap();
    for _ in 0..3 {
        assert!(window_frame(42,7,640).unwrap_err().contains("capture_permission_denied"));
    }
    assert!(click_element(42,7,"current-token",None,None,None).is_err());
    assert_eq!(std::fs::read_to_string(driver_dir().join("requests.jsonl")).unwrap(),after_action,
        "the answered action must pause later captures and pixel-capable actions before socket or CLI dispatch");
    let state=get_state(42,7,200).unwrap();
    assert_eq!(state["screenshot_error"]["code"],"capture_permission_denied");
    assert!(state.get("screenshot_data_url").is_none());
    assert!(ui_readiness::unusable_reason(&state).is_none(),"AX observation remains useful after action capture denial");

    // Bring the endpoint online only after exercising the real CLI fallback.
    // Reconnecting must not silently reset a denied endpoint's capture state.
    if cli_fallback { assert!(fixture.ensure(&cua_bin(),&socket_path().unwrap())); }
    let status=crate::permissions::status();
    assert_eq!(status.source,"driver");
    assert!(status.screen_recording && status.error.is_some(),"passive positive grants cannot clear the action denial");
    assert!(window_frame(42,7,640).unwrap_err().contains("capture_permission_denied"));
    assert!(crate::permissions::request().error.is_none(),"the explicit retry must check this endpoint's grants");
    assert!(window_frame(42,7,640).is_ok(),"capture must recover after explicit permission retry");
}

fn check_action_capture_denial_requests(rows: &[Value], cli_fallback: bool) {
    let actions:Vec<_>=rows.iter().filter(|row|row["name"]=="click").collect();
    assert_eq!(actions.len(),1,"action denial was replayed: {rows:?}");
    assert_eq!(actions[0]["transport"].as_str(),cli_fallback.then_some("cli"));
    let captures:Vec<_>=rows.iter().filter(|row|row["name"]=="get_window_state"&&row["args"]["include_screenshot"]==true).collect();
    assert_eq!(captures.len(),1,"only the explicit retry may capture: {rows:?}");
    assert!(captures[0]["transport"].is_null(),"recovery uses the now-available socket");
    let ax_reads:Vec<_>=rows.iter().filter(|row|row["name"]=="get_window_state"&&row["args"]["include_screenshot"]==false).collect();
    assert_eq!(ax_reads.len(),1);
    assert!(ax_reads[0]["args"].get("screenshot_out_file").is_none());
    assert_eq!(ax_reads[0]["transport"].as_str(),cli_fallback.then_some("cli"));
    let probes:Vec<_>=rows.iter().filter(|row|row["name"]=="check_permissions").collect();
    assert_eq!(probes.len(),2,"one passive status check and one explicit retry preflight");
    assert!(probes.iter().all(|row|row["args"]==json!({"prompt":false,"probe_direct_capture":false})));
}

#[test]
fn action_capture_denial_from_socket_pauses_previews_until_explicit_retry() {
    if std::env::var_os("ICU_OBSERVATION_CHILD").is_none() {
        let rows=child("action_capture_denial_from_socket_pauses_previews_until_explicit_retry","action-denied-direct");
        check_action_capture_denial_requests(&rows,false);
        return;
    }
    action_capture_denial_scenario(false);
}

#[test]
fn action_capture_denial_from_cli_fallback_pauses_previews_until_explicit_retry() {
    if std::env::var_os("ICU_OBSERVATION_CHILD").is_none() {
        let rows=child("action_capture_denial_from_cli_fallback_pauses_previews_until_explicit_retry","action-denied-cli");
        check_action_capture_denial_requests(&rows,true);
        return;
    }
    action_capture_denial_scenario(true);
}

#[test]
fn only_a_typed_capture_consent_denial_is_latched() {
    for code in ["px_capture_unavailable","capture_failed","window_minimized","window_not_viewable","permission_denied"] {
        assert!(confirmed_capture_denial(&json!({"screenshot_error":{"code":code,"reason":"permission denied or SCShareableContent failed"}})).is_none());
    }
    assert!(confirmed_capture_denial(&json!({"isError":true,"structuredContent":{"screenshot_error":{"code":"capture_permission_denied"}}})).is_some());
    assert!(confirmed_capture_denial(&json!({"code":"capture_permission_denied"})).is_some());
    let browser=json!({"structuredContent":{"code":"browser_route_unavailable","refusal":{"detail":{"screenshot_error":{"code":"capture_permission_denied"}}}}});
    assert_eq!(confirmed_capture_denial(&browser).unwrap()["code"],"capture_permission_denied");
    assert_eq!(browser["structuredContent"]["code"],"browser_route_unavailable","route diagnosis remains intact");
    assert!(confirmed_capture_denial(&json!({"refusal":{"detail":{"screenshot_error":{"code":"capture_failed","reason":"permission denied"}}}})).is_none());
}

#[test]
fn output_file_cannot_turn_ax_only_preflight_into_an_implicit_capture() {
    assert_eq!(request_permissions("get_window_state",&json!({"include_screenshot":false})),(true,false));
    assert_eq!(request_permissions("get_window_state",&json!({"include_screenshot":false,"screenshot_out_file":"fixture.png"})),(true,true));
    assert_eq!(request_permissions("get_window_state",&json!({"include_screenshot":true,"include_accessibility_tree":false})),(false,true));
    assert_eq!(request_permissions("health_report",&json!({})),(false,false));
    assert_eq!(request_permissions("click",&json!({"element_token":"semantic-selection"})),(true,true));
    assert_eq!(request_permissions("click",&json!({"x":1,"y":2})),(true,true));
    assert_eq!(request_permissions("drag",&json!({"from_x":1,"to_x":2})),(true,true));
    assert_eq!(request_permissions("scroll",&json!({"element_token":"semantic-scroll"})),(true,false));
    assert_eq!(request_permissions("scroll",&json!({"x":1,"y":2})),(true,true));
    assert_eq!(request_permissions("set_value",&json!({"value":"text"})),(true,false));
}

#[test]
fn direct_observation_keeps_tile_scale_capture_identity_and_readiness_evidence() {
    if std::env::var_os("ICU_OBSERVATION_CHILD").is_none() {
        let rows = child("direct_observation_keeps_tile_scale_capture_identity_and_readiness_evidence","inline");
        let reads: Vec<_> = rows.iter().filter(|r|r["name"]=="get_window_state").collect();
        assert_eq!(reads.len(),4);
        assert!(reads.iter().all(|r|r["transport"].is_null()),"no CLI process per observation: {reads:?}");
        assert!(reads.iter().filter(|r|r["args"]["include_accessibility_tree"]==true)
            .all(|r|r["args"]["screenshot_out_file"].is_string()),"agent observations preserve primary-only capture scope");
        assert!(reads.iter().filter(|r|r["args"]["include_accessibility_tree"]==false)
            .all(|r|r["args"].get("screenshot_out_file").is_none()),"tile capture contract stays inline");
        for read in reads.iter().filter(|r|r["args"]["screenshot_out_file"].is_string()) {
            assert!(!std::path::Path::new(read["args"]["screenshot_out_file"].as_str().unwrap()).exists(),"private capture file leaked");
        }
        assert!(reads.iter().all(|r|r["args"]["max_dimension"]==640 && r["args"]["include_screenshot"]==true));
        assert!(reads.iter().all(|r|r["session"]==reads[0]["session"]),"agent and tiles share the same daemon session");
        assert_eq!(reads[1]["args"]["include_accessibility_tree"],true);
        assert_eq!(reads[1]["args"]["max_elements"],200);
        return;
    }
    let before = window_frame(42,7,640).unwrap();
    let mut state = get_state(42,7,200).unwrap();
    let after = window_frame(42,7,640).unwrap();
    assert_eq!(before.2.as_deref(),Some("fixture-capture-1"));
    assert_eq!(state["screenshot_id"],"fixture-capture-2");
    assert_eq!(after.2.as_deref(),Some("fixture-capture-3"));
    assert_eq!(state["snapshot_id"],"snapshot-current");
    assert_eq!(state["elements"][0]["element_token"],"current-token");
    assert_eq!(state["elements_complete"],false,"do not manufacture exhaustive AX");
    assert_eq!(state["ax_diagnostics"]["observer_trusted"],true);
    state["task_screenshots_authorized"]=json!(true);
    let image = crate::vision::Observation::from_state(&state).unwrap().unwrap();
    assert_eq!((image.width,image.height),(32,24));
    assert_eq!(image.screenshot_id,"fixture-capture-2","later tile capture cannot replace agent identity");
    let mut busy = observation(); busy["busy"]=json!(true);
    std::fs::write(driver_dir().join("observation.json"),busy.to_string()).unwrap();
    let busy = get_state(42,7,200).unwrap();
    assert!(ui_readiness::busy(&busy));
    shutdown_owned_driver();
}

#[test]
fn direct_observation_does_not_capture_inline_auxiliary_surfaces() {
    if std::env::var_os("ICU_OBSERVATION_CHILD").is_none() {
        let rows=child("direct_observation_does_not_capture_inline_auxiliary_surfaces","primary-only");
        let reads:Vec<_>=rows.iter().filter(|row|row["name"]=="get_window_state").collect();
        assert_eq!(reads.len(),2);
        assert!(reads.iter().all(|row|row["transport"].is_null()),"both calls use the direct socket");
        assert!(reads[0]["args"].get("screenshot_out_file").is_none(),"control proves inline overlay fixture");
        assert!(reads[1]["args"]["screenshot_out_file"].is_string());
        return;
    }
    std::fs::write(driver_dir().join("auxiliary-surface"),"").unwrap();
    window_frame(42,7,640).unwrap();
    let before=std::fs::read_to_string(driver_dir().join("auxiliary-captures")).unwrap();
    let state=get_state(42,7,200).unwrap();
    assert!(state["screenshot_data_url"].is_string());
    assert_eq!(state["screenshot_id"],"fixture-capture-2");
    assert!(state.get("screenshots").is_none(),"no auxiliary surface pixels reach the observation");
    assert_eq!(std::fs::read_to_string(driver_dir().join("auxiliary-captures")).unwrap(),before,
        "moving to direct sockets must not broaden the captured surface set");
    shutdown_owned_driver();
}

#[test]
fn observation_files_reject_unrelated_paths_and_failed_capture_pixels() {
    let file=CaptureFile::new().unwrap();
    std::fs::write(file.path(),b"fixture image").unwrap();
    let mut unrelated=json!({"screenshot_file_path":"/unrelated.png","screenshot_data_url":"stale"});
    attach_observation_file(&mut unrelated,&file.path());
    assert!(unrelated.get("screenshot_data_url").is_none());
    let mut failed=json!({"screenshot_file_path":file.path(),"screenshot_frame_valid":false,"screenshot_data_url":"stale"});
    attach_observation_file(&mut failed,&file.path());
    assert!(failed.get("screenshot_data_url").is_none());
    let mut valid=json!({"screenshot_file_path":file.path(),"screenshot_id":"current"});
    attach_observation_file(&mut valid,&file.path());
    assert!(valid["screenshot_data_url"].is_string());
    assert_eq!(valid["screenshot_id"],"current");
}

#[test]
fn canvas_readiness_still_requires_an_authorized_current_image() {
    if std::env::var_os("ICU_OBSERVATION_CHILD").is_none() {
        child("canvas_readiness_still_requires_an_authorized_current_image","canvas");
        return;
    }
    let mut canvas = observation();
    canvas["elements"]=json!([]); canvas["passive_elements"]=json!([]);
    canvas["degraded"]=json!(true); canvas["degraded_reason"]=json!("ax_tree_empty");
    std::fs::write(driver_dir().join("observation.json"),canvas.to_string()).unwrap();
    let mut state = get_state(42,7,200).unwrap();
    assert!(ui_readiness::unusable_reason(&state).is_some());
    assert!(crate::vision::Observation::from_state(&state).unwrap().is_none(),"capture does not authorize model sharing");
    state["task_screenshots_authorized"]=json!(true);
    let valid = crate::vision::Observation::from_state(&state).unwrap().is_some();
    assert!(valid);
    assert!(ui_readiness::unusable_reason_with_image(&state,valid).is_none());
    canvas["partial"]=json!(true);
    std::fs::write(driver_dir().join("observation.json"),canvas.to_string()).unwrap();
    let mut state = get_state(42,7,200).unwrap(); state["task_screenshots_authorized"]=json!(true);
    let valid = crate::vision::Observation::from_state(&state).unwrap().is_some();
    assert!(ui_readiness::unusable_reason_with_image(&state,valid).is_some(),"a valid image cannot override partial evidence");
    shutdown_owned_driver();
}

#[test]
fn a_missing_image_does_not_reuse_a_prior_capture_or_retry_the_observation() {
    if std::env::var_os("ICU_OBSERVATION_CHILD").is_none() {
        let rows = child("a_missing_image_does_not_reuse_a_prior_capture_or_retry_the_observation","missing");
        let reads: Vec<_> = rows.iter().filter(|r|r["name"]=="get_window_state").collect();
        assert_eq!(reads.len(),2);
        assert!(reads.iter().all(|r|r["transport"].is_null()));
        return;
    }
    assert!(get_state(42,7,200).unwrap()["screenshot_data_url"].is_string());
    std::fs::write(driver_dir().join("capture-refusal.json"),json!({
        "structuredContent":observation(),"content":[]}).to_string()).unwrap();
    let mut next = get_state(42,7,200).unwrap(); next["task_screenshots_authorized"]=json!(true);
    assert!(next.get("screenshot_data_url").is_none());
    assert!(crate::vision::Observation::from_state(&next).is_err());
    shutdown_owned_driver();
}

#[test]
fn completed_refusals_and_incompatible_metadata_do_not_fall_back_to_cli() {
    if let Ok(case)=std::env::var("ICU_OBSERVATION_CHILD") {
        let error=get_state(42,7,200).unwrap_err();
        shutdown_owned_driver();
        assert!(error.contains(if case=="incompatible" {"incompatible"} else {"permission_denied"}),"{error}");
        return;
    }
    for case in ["refused","structured-error","incompatible"] {
        let rows=child("completed_refusals_and_incompatible_metadata_do_not_fall_back_to_cli",case);
        assert!(!rows.iter().any(|r|r["transport"]=="cli"),"{case}: {rows:?}");
        assert_eq!(rows.iter().filter(|r|r["name"]=="get_window_state").count(),usize::from(case!="incompatible"));
    }
}

#[test]
fn unavailable_direct_transport_keeps_the_cli_observation_fallback() {
    if std::env::var_os("ICU_OBSERVATION_CHILD").is_none() {
        let rows=child("unavailable_direct_transport_keeps_the_cli_observation_fallback","unavailable");
        assert_eq!(rows.len(),1,"{rows:?}");
        assert_eq!(rows[0]["transport"],"cli");
        assert!(rows[0]["args"]["screenshot_out_file"].is_string());
        return;
    }
    let mut state=get_state(42,7,200).unwrap(); state["task_screenshots_authorized"]=json!(true);
    let image=crate::vision::Observation::from_state(&state).unwrap().unwrap();
    assert_eq!(image.screenshot_id,"fixture-capture-1");
    assert_eq!((image.width,image.height),(32,24));
    shutdown_owned_driver();
}

#[test]
fn interleaved_preview_never_rebinds_agent_pixels_or_stale_element_tokens() {
    if std::env::var_os("ICU_OBSERVATION_CHILD").is_none() {
        let rows=child("interleaved_preview_never_rebinds_agent_pixels_or_stale_element_tokens","interleaved");
        let clicks:Vec<_>=rows.iter().filter(|r|r["name"]=="click").collect();
        assert_eq!(clicks.len(),4,"each refusal or delivery is attempted exactly once");
        assert_eq!(clicks[0]["args"]["screenshot_id"],"fixture-capture-1","never silently use tile capture 2");
        assert_eq!(clicks[1]["args"]["element_token"],"fixture-token-1","never retarget the stale token");
        assert_eq!(clicks[2]["args"]["screenshot_id"],"fixture-capture-3");
        assert_eq!(clicks[3]["args"]["element_token"],"fixture-token-3");
        assert!(!rows.iter().any(|r|r["transport"]=="cli"),"stale replies never trigger CLI replay");
        return;
    }
    let dir=driver_dir();
    std::fs::write(dir.join("strict-observation-bindings"),"").unwrap();
    let old=get_state(42,7,200).unwrap();
    let tile=window_frame(42,7,640).unwrap();
    assert_eq!(old["snapshot_id"],"fixture-snapshot-1");
    assert_eq!(tile.2.as_deref(),Some("fixture-capture-2"));
    let pixel=click_pixel_with_delivery(42,7,10.,12.,"left",1,observed_screenshot_id(&old).unwrap(),false).unwrap_err();
    assert!(pixel.to_string().contains("stale_capture"),"{pixel}");
    let element=click_element(42,7,old["elements"][0]["element_token"].as_str().unwrap(),None,None,None).unwrap_err();
    assert!(element.to_string().contains("stale_element_token"),"{element}");
    assert!(!dir.join("delivered-clicks.jsonl").exists(),"nothing delivered against the new tile's state");
    let fresh=get_state(42,7,200).unwrap();
    assert_eq!(fresh["snapshot_id"],"fixture-snapshot-3");
    click_pixel_with_delivery(42,7,10.,12.,"left",1,observed_screenshot_id(&fresh).unwrap(),false).unwrap();
    click_element(42,7,fresh["elements"][0]["element_token"].as_str().unwrap(),None,None,None).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("delivered-clicks.jsonl")).unwrap().lines().count(),2);
    shutdown_owned_driver();
}
