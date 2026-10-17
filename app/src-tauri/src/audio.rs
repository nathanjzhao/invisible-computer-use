//! Explicit local audio capture. Audio never enters task memory or a model request.
use serde::{Deserialize, Serialize};
use serde_json::{json,Value};
use std::{path::{Path,PathBuf},sync::{Arc,Mutex,atomic::{AtomicBool,Ordering}},time::{Duration,Instant}};

#[derive(Clone,Debug,Deserialize,Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request { pub microphone:bool, pub system:bool, pub seconds:f64, pub consent:bool }
impl Request {
    fn milliseconds(&self)->Result<u64,String>{
        let millis=self.seconds*1000.;
        if !millis.is_finite() || !(100.0..=3_600_000.0).contains(&millis) || (millis-millis.round()).abs()>0.000001 {
            return Err("Recording duration must be 0.1 to 3600 seconds, in whole milliseconds".into());
        }
        Ok(millis.round() as u64)
    }
    fn source(&self)->Result<&'static str,String>{
        if !self.consent {return Err("Confirm the selected audio sources before recording".into());}
        self.milliseconds()?;
        match (self.microphone,self.system) {(true,false)=>Ok("microphone"),(false,true)=>Ok("system"),(true,true)=>Ok("both"),_=>Err("Choose at least one audio source".into())}
    }
}
struct Active {stop:Arc<AtomicBool>,status:Value}
#[derive(Default)]
pub struct Service { inner:Arc<Mutex<Option<Active>>>, closed:AtomicBool }
fn helper()->Result<PathBuf,String>{
    let name=if cfg!(windows){"taiku-observer.exe"}else{"taiku-observer"};
    let path=std::env::var_os("ICU_OBSERVER_BIN").map(PathBuf::from).or_else(||std::env::current_exe().ok().and_then(|p|p.parent().map(|p|p.join(name)))).ok_or("Audio recorder is unavailable")?;
    if !path.is_file(){return Err("Install the audio-enabled recorder beside the application before recording".into());}
    Ok(path)
}
impl Service {
    pub fn status(&self)->Value {self.inner.lock().unwrap_or_else(|p|p.into_inner()).as_ref().map(|a|a.status.clone()).unwrap_or(json!({"state":"idle"}))}
    pub fn start(&self,root:&Path,request:Request)->Result<Value,String>{
        let source=request.source()?;
        let milliseconds=request.milliseconds()?;
        let mut slot=self.inner.lock().unwrap_or_else(|p|p.into_inner());
        if self.closed.load(Ordering::SeqCst){return Err("The app is shutting down".into());}
        if slot.as_ref().is_some_and(|a|matches!(a.status["state"].as_str(),Some("recording"|"stopping"))){return Err("An audio recording is already active".into());}
        let helper=helper()?;
        std::fs::create_dir_all(root).map_err(|e|format!("Cannot create audio folder: {e}"))?;
        #[cfg(unix)] { use std::os::unix::fs::PermissionsExt;std::fs::set_permissions(root,std::fs::Permissions::from_mode(0o700)).map_err(|e|e.to_string())?; }
        let stop=Arc::new(AtomicBool::new(false));
        let status=json!({"state":"recording","microphone":request.microphone,"system":request.system,"max_seconds":request.seconds});
        *slot=Some(Active {stop:stop.clone(),status:status.clone()});
        let inner=self.inner.clone();let root=root.to_path_buf();
        let launched=std::thread::Builder::new().name("audio-recording".into()).spawn(move||{
            // Virtual audio inputs need not enforce microphone TCC themselves.
            // Ask as the app that owns the consent, before starting any sensor.
            #[cfg(target_os = "macos")]
            let permission=if request.microphone {crate::audio_macos::authorize(&stop)} else {Ok(())};
            #[cfg(not(target_os = "macos"))]
            let permission:Result<(),String>=Ok(());
            let result=permission.and_then(|()|crate::audio_supervisor::capture(&helper,&root,source,milliseconds,stop));
            let result=result.and_then(|output|{
                if !output.status.success(){return Err(format!("Audio recorder stopped without completing: {}",String::from_utf8_lossy(&output.stderr).chars().take(500).collect::<String>()));}
                let receipt=String::from_utf8_lossy(&output.stdout).lines().filter_map(|line|serde_json::from_str::<Value>(line).ok()).find(|v|v["finalized"]==true).ok_or("Audio recorder did not finalize its output")?;
                let directory=receipt["recording_directory"].as_str().ok_or("Audio recorder did not return a folder")?;
                let directory=Path::new(directory).canonicalize().map_err(|e|e.to_string())?;
                if !directory.starts_with(root.canonicalize().map_err(|e|e.to_string())?){return Err("Audio recorder returned a folder outside its output root".into());}
                inspect(&directory,&request)
            });
            let status=match result {Ok(status)=>status,Err(error)=>json!({"state":"failed","error":error})};
            if let Some(active)=inner.lock().unwrap_or_else(|p|p.into_inner()).as_mut(){active.status=status;}
        });
        if let Err(error)=launched {*slot=None;return Err(format!("Cannot start audio worker: {error}"));}
        Ok(status)
    }
    pub fn stop(&self)->Value {
        let mut slot=self.inner.lock().unwrap_or_else(|p|p.into_inner());
        if let Some(active)=slot.as_mut(){if active.status["state"]=="recording"{active.stop.store(true,Ordering::SeqCst);active.status["state"]=json!("stopping");}active.status.clone()}else{json!({"state":"idle"})}
    }
    pub fn shutdown(&self){self.closed.store(true,Ordering::SeqCst);self.stop();let deadline=Instant::now()+Duration::from_secs(9);while Instant::now()<deadline&&matches!(self.status()["state"].as_str(),Some("recording"|"stopping")){std::thread::sleep(Duration::from_millis(25));}}
}
fn inspect(directory:&Path,request:&Request)->Result<Value,String>{
    let metadata=std::fs::metadata(directory.join("manifest.json")).map_err(|e|e.to_string())?;
    if metadata.len()>1024*1024{return Err("Audio manifest is too large".into());}
    let manifest:Value=serde_json::from_slice(&std::fs::read(directory.join("manifest.json")).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    let sources=manifest.pointer("/audio/sources").and_then(Value::as_array).cloned().unwrap_or_default();
    for source in &sources {match source["name"].as_str(){Some("mic") if request.microphone=>{},Some("system") if request.system=>{},_=>return Err("Audio recorder produced an unselected source".into())}}
    let captured=|name:&str|sources.iter().any(|s|s["name"]==name&&s["duration_sec"].as_f64().is_some_and(|d|d>0.));
    let missing:Vec<_>=[("mic",request.microphone),("system",request.system)].into_iter().filter(|(name,selected)|*selected&&!captured(name)).map(|(name,_)|name).collect();
    // Linux preserves samples captured before a device fails. A finalized partial
    // file must not make an interrupted source appear fully saved in the UI.
    #[cfg(target_os="linux")]
    let missing = {
        let mut missing = missing;
        let events = directory.join("events.jsonl");
        if events.exists() {
            if std::fs::metadata(&events).map_err(|e|e.to_string())?.len()>1024*1024 {
                return Err("Audio event log is too large".into());
            }
            for line in std::fs::read_to_string(events).map_err(|e|e.to_string())?.lines() {
                let event:Value=serde_json::from_str(line).map_err(|e|e.to_string())?;
                if event["kind"]=="audio.source_error" {
                    for (name,selected) in [("mic",request.microphone),("system",request.system)] {
                        if selected && event["payload"]["name"]==name && !missing.contains(&name) {missing.push(name);}
                    }
                }
            }
        }
        missing
    };
    let any_captured=captured("mic")||captured("system");
    Ok(json!({"state":if missing.is_empty(){"saved"}else if !any_captured{"unavailable"}else{"partial"},"directory":display_path(&directory.to_string_lossy()),"sources":sources,"unavailable_sources":missing}))
}
pub(crate) fn display_path(path:&str)->String {
    if let Some(unc)=path.strip_prefix(r"\\?\UNC\"){return format!(r"\\{unc}");}
    path.strip_prefix(r"\\?\").unwrap_or(path).to_string()
}

#[cfg(test)] mod tests {
    use super::*;
    #[cfg(target_os="linux")]
    #[test]fn interrupted_linux_source_keeps_samples_but_is_partial(){
        let dir=std::env::temp_dir().join(format!("icu-audio-interrupted-{}",std::process::id()));std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("manifest.json"),br#"{"audio":{"sources":[{"name":"mic","duration_sec":2}]}}"#).unwrap();
        std::fs::write(dir.join("events.jsonl"),br#"{"kind":"audio.source_error","payload":{"name":"mic","error":"disconnected"}}"#).unwrap();
        let request=Request{microphone:true,system:false,seconds:4.0,consent:true};
        let status=inspect(&dir,&request).unwrap();assert_eq!(status["state"],"partial");assert_eq!(status["unavailable_sources"],json!(["mic"]));assert_eq!(status["sources"][0]["duration_sec"],2);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]fn sources_need_fresh_explicit_consent(){
        let mut q=Request{microphone:false,system:true,seconds:60.,consent:false};assert!(q.source().is_err());q.consent=true;assert_eq!(q.source().unwrap(),"system");q.system=false;assert!(q.source().is_err());q.microphone=true;assert_eq!(q.source().unwrap(),"microphone");q.seconds=3601.;assert!(q.source().is_err());
    }
    #[test]fn empty_recording_is_unavailable_and_windows_paths_are_readable(){
        let dir=std::env::temp_dir().join(format!("icu-audio-empty-{}",std::process::id()));std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("manifest.json"),br#"{"audio":{"sources":[{"name":"system","duration_sec":0}]}}"#).unwrap();
        let request=Request{microphone:false,system:true,seconds:1.,consent:true};
        assert_eq!(inspect(&dir,&request).unwrap()["state"],"unavailable");
        assert_eq!(display_path(r"\\?\C:\Users\Test\audio"),r"C:\Users\Test\audio");
        assert_eq!(display_path(r"\\?\UNC\server\share"),r"\\server\share");
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]fn subsecond_limits_preserve_millisecond_precision(){
        let mut request=Request{microphone:false,system:true,seconds:0.1,consent:true};
        assert_eq!(request.milliseconds().unwrap(),100);
        request.seconds=1.001;assert_eq!(request.milliseconds().unwrap(),1001);
        request.seconds=3600.;assert_eq!(request.milliseconds().unwrap(),3_600_000);
        for value in [0.,0.099,0.1005,3600.001,f64::NAN,f64::INFINITY] {request.seconds=value;assert!(request.source().is_err());}
    }
}
