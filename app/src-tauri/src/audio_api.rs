//! Opt-in stdio audio tools in the signed app. No GUI or ambient workers start.
use std::{io::{BufRead, Write}, path::PathBuf, sync::mpsc, time::{Duration, Instant}};
use serde_json::{json, Value};
use base64::Engine;

const FLAG: &str = "--icu-audio-api";
const LIMIT_ERROR: &str = "audio recording duration must be an integer from 100 through 300000";
const MAX_LINE: u64 = 1024 * 1024;

trait Capture {
    fn start(&mut self, milliseconds: u64) -> Result<(), String>;
    fn stop(&mut self);
    fn status(&self) -> Value;
    fn finish(&mut self) -> Result<Value, String>;
    fn shutdown(&mut self);
}
struct Native { service: crate::audio::Service, root: PathBuf }
impl Capture for Native {
    fn start(&mut self, milliseconds: u64) -> Result<(), String> {
        self.service.start(&self.root, crate::audio::Request { microphone:false, system:true,
            seconds:milliseconds as f64 / 1000., consent:true }).map(|_|())
    }
    fn stop(&mut self) { self.service.stop(); }
    fn status(&self) -> Value { self.service.status() }
    fn finish(&mut self) -> Result<Value, String> {
        let export=crate::audio_wav::export(&self.root,&self.service.status())?;
        if export.files.len()!=1 || export.files[0].source!="system" {return Err("System audio did not produce a complete recording".into());}
        let path=PathBuf::from(&export.files[0].filepath);
        let mut options=std::fs::OpenOptions::new();options.read(true);
        #[cfg(unix)] {use std::os::unix::fs::OpenOptionsExt;options.custom_flags(libc::O_NOFOLLOW|libc::O_NONBLOCK);}
        #[cfg(windows)] {use std::os::windows::fs::OpenOptionsExt;options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);}
        let mut file=options.open(&path).map_err(|e|e.to_string())?;
        let metadata=file.metadata().map_err(|e|e.to_string())?;
        #[cfg(windows)] {use std::os::windows::fs::MetadataExt;if metadata.file_attributes()&windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT!=0{return Err("Recorded WAV changed to a link".into());}}
        if !metadata.is_file() || metadata.len()>32*1024*1024 {return Err("Recorded WAV exceeds the audio API result limit".into());}
        let mut bytes=Vec::new();use std::io::Read;
        (&mut file).take(32*1024*1024+1).read_to_end(&mut bytes).map_err(|e|e.to_string())?;
        if bytes.len()>32*1024*1024 {return Err("Recorded WAV changed during export".into());}
        Ok(json!({"filepath":export.files[0].filepath,"data_url":format!("data:audio/wav;base64,{}",base64::engine::general_purpose::STANDARD.encode(bytes))}))
    }
    fn shutdown(&mut self) {self.service.shutdown();}
}
fn duration(args:&Value)->Result<u64,String>{
    let object=args.as_object().ok_or("Audio arguments must be an object")?;
    if object.keys().any(|k|k!="max_duration_ms") {return Err("Unknown audio recording argument".into());}
    let value=match object.get("max_duration_ms") {None|Some(Value::Null)=>60_000.,Some(value)=>value.as_f64().ok_or(LIMIT_ERROR)?};
    if !value.is_finite() || value.fract()!=0. || !(100.0..=300_000.0).contains(&value) {return Err(LIMIT_ERROR.into());}
    Ok(value as u64)
}
fn result(id:Value,value:Value)->Value {json!({"jsonrpc":"2.0","id":id,"result":value})}
fn tool_result(id:Value,value:Value)->Value {result(id,json!({"content":[{"type":"text","text":"Audio operation completed."}],"structuredContent":value}))}
fn refused(id:Value,message:impl Into<String>)->Value {result(id,json!({"isError":true,"content":[{"type":"text","text":message.into()}]}))}
struct Approval { call:Value, id:String, milliseconds:u64, expires:Instant }
struct PendingStop { id:Value, expires:Instant }
struct Protocol<B:Capture> {
    backend:B, initialized:bool, elicitation:bool, approval:Option<Approval>, stop:Option<PendingStop>,
    recording:bool, start_id:Option<Value>, sequence:u64,
}
impl<B:Capture> Protocol<B> {
    fn new(backend:B)->Self {Self {backend,initialized:false,elicitation:false,approval:None,stop:None,recording:false,start_id:None,sequence:0}}
    fn cancel_approval(&mut self)->Vec<Value>{
        self.approval.take().map(|p|vec![json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":p.id,"reason":"Audio request cancelled"}}),refused(p.call,"Audio recording was cancelled")]).unwrap_or_default()
    }
    fn handle(&mut self,message:Value)->Vec<Value>{
        let id=message.get("id").cloned();
        if message.get("method").is_none(){
            if self.approval.as_ref().is_some_and(|p|id.as_ref()==Some(&json!(p.id))) {
                let p=self.approval.take().unwrap();
                if Instant::now()>=p.expires || message.pointer("/result/action")!=Some(&json!("accept")) {
                    return vec![refused(p.call,"Computer audio recording was not approved")];
                }
                return vec![match self.backend.start(p.milliseconds) {
                    Ok(())=>{self.recording=true;self.start_id=Some(p.call.clone());tool_result(p.call,json!({"state":"recording","max_duration_ms":p.milliseconds}))},
                    Err(error)=>refused(p.call,error),
                }];
            }
            return vec![];
        }
        if message["method"]=="notifications/cancelled" {
            let cancelled=message.pointer("/params/requestId");
            if self.approval.as_ref().is_some_and(|p|cancelled==Some(&p.call)){return self.cancel_approval();}
            if cancelled.is_some() && (cancelled==self.start_id.as_ref() || self.stop.as_ref().is_some_and(|p|cancelled==Some(&p.id))) {
                self.backend.stop();
                if let Some(p)=self.stop.take(){return vec![refused(p.id,"Audio result request was cancelled")];}
            }
            return vec![];
        }
        let Some(id)=id else {return vec![]};
        if !matches!(id,Value::String(_)|Value::Number(_)) || message["jsonrpc"]!="2.0" {
            return vec![json!({"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid request"}})];
        }
        match message["method"].as_str() {
            Some("initialize")=>{
                if self.initialized{return vec![refused(id,"Audio connection is already initialized")];}
                self.initialized=true;self.elicitation=message.pointer("/params/capabilities/elicitation").is_some_and(Value::is_object);
                vec![result(id,json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"copilot-audio","version":"0.1.0"},
                    "instructions":"Explicit system-audio recording only. Start requires client-mediated approval for each recording. Stop returns a local WAV path and data URL. The bundled JavaScript adapter also returns bytes. Disconnect cancels capture. No ambient history, images or microphone are collected by this API."}))]
            }
            Some("ping")=>vec![result(id,json!({}))],
            _ if !self.initialized=>vec![refused(id,"Initialize the audio connection first")],
            Some("tools/list")=>vec![result(id,json!({"tools":[
                {"name":"start_audio_recording","description":"Record system audio for this task after a fresh approval. Defaults to 60000 ms.","inputSchema":{"type":"object","properties":{"max_duration_ms":{"type":"integer","minimum":100,"maximum":300000}},"additionalProperties":false}},
                {"name":"stop_audio_recording","description":"Stop and finalize system audio; return filepath and audio/wav data_url.","inputSchema":{"type":"object","properties":{},"additionalProperties":false}}
            ]}))],
            Some("tools/call")=>{
                let args=message.pointer("/params/arguments").cloned().unwrap_or(json!({}));
                match message.pointer("/params/name").and_then(Value::as_str) {
                    Some("start_audio_recording")=>{
                        let milliseconds=match duration(&args){Ok(n)=>n,Err(e)=>return vec![refused(id,e)]};
                        if !self.elicitation{return vec![refused(id,"This audio client cannot request recording approval")];}
                        if self.approval.is_some()||self.stop.is_some()||self.recording{return vec![refused(id,"Finish the current audio request first")];}
                        self.sequence+=1;let approval_id=format!("audio-approval-{}",self.sequence);
                        self.approval=Some(Approval{call:id,id:approval_id.clone(),milliseconds,expires:Instant::now()+Duration::from_secs(90)});
                        vec![json!({"jsonrpc":"2.0","id":approval_id,"method":"elicitation/create","params":{"message":format!("Allow this task to record system audio for up to {} seconds and receive the resulting WAV?",milliseconds as f64/1000.),"requestedSchema":{"type":"object","properties":{},"additionalProperties":false},"_meta":{"riskLevel":"high","tool_name":"start_audio_recording"}}})]
                    }
                    Some("stop_audio_recording")=>{
                        if !args.as_object().is_some_and(|o|o.is_empty()){return vec![refused(id,"Stop takes no arguments")];}
                        if self.approval.is_some(){let mut replies=self.cancel_approval();replies.push(refused(id,"No approved recording is active"));return replies;}
                        if !self.recording||self.stop.is_some(){return vec![refused(id,"No new audio recording result is available")];}
                        self.backend.stop();self.stop=Some(PendingStop{id,expires:Instant::now()+Duration::from_secs(18)});self.tick()
                    }
                    _=>vec![refused(id,"Unknown audio tool")],
                }
            }
            _=>vec![json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Method not found"}})],
        }
    }
    fn tick(&mut self)->Vec<Value>{
        if self.approval.as_ref().is_some_and(|p|Instant::now()>=p.expires){return self.cancel_approval();}
        let Some(p)=self.stop.as_ref() else{return vec![]};
        let status=self.backend.status();
        if matches!(status["state"].as_str(),Some("recording"|"stopping")) && Instant::now()<p.expires{return vec![];}
        let p=self.stop.take().unwrap();self.recording=false;self.start_id=None;
        let value=if matches!(status["state"].as_str(),Some("saved"|"partial")){self.backend.finish()}
                  else {self.backend.stop();Err(status["error"].as_str().unwrap_or("System audio is unavailable or did not finish").to_owned())};
        vec![match value {Ok(value)=>tool_result(p.id,value),Err(error)=>refused(p.id,error)}]
    }
}
impl<B:Capture> Drop for Protocol<B>{fn drop(&mut self){self.backend.shutdown();}}

fn root()->Result<PathBuf,String>{
    if let Some(path)=std::env::var_os("ICU_AUDIO_ROOT") {
        let path=PathBuf::from(path);return path.is_absolute().then_some(path).ok_or("ICU_AUDIO_ROOT must be absolute".into());
    }
    #[cfg(windows)] let data=PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("Local application data is unavailable")?);
    #[cfg(target_os="macos")] let data=PathBuf::from(std::env::var_os("HOME").ok_or("Home directory is unavailable")?).join("Library/Application Support");
    #[cfg(not(any(windows,target_os="macos")))] let data=std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|p|p.is_absolute())
        .unwrap_or(PathBuf::from(std::env::var_os("HOME").ok_or("Home directory is unavailable")?).join(".local/share"));
    if !data.is_absolute(){return Err("Audio storage root is not absolute".into());}
    Ok(data.join("com.nathanzhao.app/audio"))
}
fn serve()->Result<(),String>{
    let mut protocol=Protocol::new(Native{service:Default::default(),root:root()?});
    let (sender,receiver)=mpsc::sync_channel(16);
    std::thread::spawn(move||{
        let input=std::io::stdin();let mut input=input.lock();
        loop {
            use std::io::Read;
            let mut line=Vec::new();
            match input.by_ref().take(MAX_LINE+1).read_until(b'\n',&mut line) {
                Ok(0)|Err(_)=>break,
                Ok(_) if line.len() as u64>MAX_LINE=>break,
                Ok(_)=>if sender.send(line).is_err(){break;},
            }
        }
    });
    loop {
        let mut replies=match receiver.recv_timeout(Duration::from_millis(20)) {
            Ok(line)=>match serde_json::from_slice::<Value>(&line) {
                Ok(message)=>protocol.handle(message),
                Err(_)=>vec![json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Invalid JSON"}})],
            },
            Err(mpsc::RecvTimeoutError::Timeout)=>protocol.tick(),
            Err(mpsc::RecvTimeoutError::Disconnected)=>break,
        };
        replies.extend(protocol.tick());
        let stdout=std::io::stdout();let mut output=stdout.lock();
        for reply in replies {
            serde_json::to_writer(&mut output,&reply).map_err(|e|e.to_string())?;
            output.write_all(b"\n").map_err(|e|e.to_string())?;output.flush().map_err(|e|e.to_string())?;
        }
    }
    Ok(())
}
pub fn entry()->Option<i32>{
    if std::env::args_os().nth(1).as_deref()!=Some(std::ffi::OsStr::new(FLAG)){return None;}
    if std::env::args_os().count()!=2 {eprintln!("Audio API takes no command arguments");return Some(1);}
    Some(match serve(){Ok(())=>0,Err(error)=>{eprintln!("Audio API: {error}");1}})
}

#[cfg(test)] mod tests {
    use super::*;
    #[derive(Default)]struct Fake { starts:Vec<u64>, stopped:bool, shutdown:std::sync::Arc<std::sync::atomic::AtomicBool> }
    impl Capture for Fake {
        fn start(&mut self,n:u64)->Result<(),String>{self.starts.push(n);self.stopped=false;Ok(())}
        fn stop(&mut self){self.stopped=true;}
        fn status(&self)->Value{json!({"state":if self.stopped{"saved"}else{"recording"}})}
        fn finish(&mut self)->Result<Value,String>{Ok(json!({"filepath":"/fixture/audio.wav","data_url":"data:audio/wav;base64,UklGRg=="}))}
        fn shutdown(&mut self){self.stopped=true;self.shutdown.store(true,std::sync::atomic::Ordering::SeqCst);}
    }
    fn call(id:u64,name:&str,args:Value)->Value{json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}})}
    fn initialized()->Protocol<Fake>{let mut p=Protocol::new(Fake::default());p.handle(json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"capabilities":{"elicitation":{}}}}));p}
    #[test]fn reference_duration_defaults_and_bounds_are_exact(){
        assert_eq!(duration(&json!({})).unwrap(),60000);assert_eq!(duration(&json!({"max_duration_ms":null})).unwrap(),60000);
        for n in [100,1001,300000]{assert_eq!(duration(&json!({"max_duration_ms":n})).unwrap(),n);}
        for n in [json!(-1),json!(99),json!(300001),json!(100.5),json!("100"),json!(true)]{assert_eq!(duration(&json!({"max_duration_ms":n})).unwrap_err(),LIMIT_ERROR);}
    }
    #[test]fn no_device_starts_without_the_exact_current_approval(){
        let mut p=initialized();let request=p.handle(call(1,"start_audio_recording",json!({"max_duration_ms":100})));
        assert_eq!(request[0]["method"],"elicitation/create");assert!(p.backend.starts.is_empty());
        p.handle(json!({"jsonrpc":"2.0","id":"unrelated","result":{"action":"accept"}}));assert!(p.backend.starts.is_empty());
        p.handle(json!({"jsonrpc":"2.0","id":request[0]["id"],"result":{"action":"decline"}}));assert!(p.backend.starts.is_empty());
        let next=p.handle(call(2,"start_audio_recording",json!({})));
        p.handle(json!({"jsonrpc":"2.0","id":request[0]["id"],"result":{"action":"accept"}}));assert!(p.backend.starts.is_empty());
        p.handle(json!({"jsonrpc":"2.0","id":next[0]["id"],"result":{"action":"accept"}}));assert_eq!(p.backend.starts,vec![60000]);
    }
    #[test]fn cancellation_cannot_be_revived_by_a_late_accept(){
        let mut p=initialized();let request=p.handle(call(1,"start_audio_recording",json!({})));
        p.handle(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}));
        p.handle(json!({"jsonrpc":"2.0","id":request[0]["id"],"result":{"action":"accept"}}));assert!(p.backend.starts.is_empty());
    }
    #[test]fn approval_is_per_recording_and_stop_returns_one_result(){
        let mut p=initialized();let request=p.handle(call(1,"start_audio_recording",json!({})));
        p.handle(json!({"jsonrpc":"2.0","id":request[0]["id"],"result":{"action":"accept"}}));
        assert_eq!(p.handle(call(2,"start_audio_recording",json!({})))[0]["result"]["isError"],true);
        let stopped=p.handle(call(3,"stop_audio_recording",json!({})));assert_eq!(stopped[0]["result"]["structuredContent"]["filepath"],"/fixture/audio.wav");
        assert_eq!(p.handle(call(4,"stop_audio_recording",json!({})))[0]["result"]["isError"],true);
        assert_eq!(p.handle(call(5,"start_audio_recording",json!({})))[0]["method"],"elicitation/create");assert_eq!(p.backend.starts.len(),1);
    }
    #[test]fn unsupported_clients_and_invalid_limits_fail_before_approval(){
        let mut p=Protocol::new(Fake::default());p.handle(json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}));
        assert_eq!(p.handle(call(1,"start_audio_recording",json!({})))[0]["result"]["isError"],true);
        let mut p=initialized();assert_eq!(p.handle(call(1,"start_audio_recording",json!({"max_duration_ms":99})))[0]["result"]["isError"],true);assert!(p.approval.is_none());
    }
    #[test]fn expired_approvals_and_connection_drop_never_leave_capture_owned(){
        let mut p=initialized();let shutdown=p.backend.shutdown.clone();
        let request=p.handle(call(1,"start_audio_recording",json!({})));
        p.approval.as_mut().unwrap().expires=Instant::now()-Duration::from_secs(1);
        p.handle(json!({"jsonrpc":"2.0","id":request[0]["id"],"result":{"action":"accept"}}));
        assert!(p.backend.starts.is_empty());
        let request=p.handle(call(2,"start_audio_recording",json!({})));
        p.handle(json!({"jsonrpc":"2.0","id":request[0]["id"],"result":{"action":"accept"}}));
        assert_eq!(p.backend.starts.len(),1);drop(p);
        assert!(shutdown.load(std::sync::atomic::Ordering::SeqCst));
    }
}
