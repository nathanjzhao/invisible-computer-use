//! Enforce the app's microphone grant even for virtual input devices.
use std::{sync::{atomic::{AtomicBool, Ordering}, mpsc}, time::{Duration, Instant}};
use block2::RcBlock;
use objc2::{class, msg_send, runtime::Bool};
use objc2_foundation::NSString;

#[link(name = "AVFoundation", kind = "framework")]
extern "C" { static AVMediaTypeAudio: *const NSString; }

const DENIED: &str = "Microphone access is off. Enable Copilot in System Settings > Privacy & Security > Microphone, then quit and reopen the app. You can also deselect Microphone to record system sound only.";

#[derive(Debug, PartialEq)]
enum Permission { Ask, Granted, Denied }
fn permission(status: isize) -> Permission {
    match status { 0 => Permission::Ask, 3 => Permission::Granted, _ => Permission::Denied }
}

fn await_grant(receiver: mpsc::Receiver<bool>, stop: &AtomicBool, timeout: Duration) -> Result<(), String> {
    let deadline=Instant::now()+timeout;
    loop {
        if stop.load(Ordering::SeqCst) {return Err("Audio recording was cancelled before microphone access was granted".into());}
        let remaining=deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {return Err("Microphone permission was not answered. Try recording again when ready.".into());}
        match receiver.recv_timeout(remaining.min(Duration::from_millis(50))) {
            Ok(true) if !stop.load(Ordering::SeqCst) => return Ok(()),
            Ok(_) => return Err(DENIED.into()),
            Err(mpsc::RecvTimeoutError::Timeout) => {},
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err("Microphone permission could not be checked".into()),
        }
    }
}

/// Runs only after the explicit audio Start action, on the recording worker.
pub fn authorize(stop: &AtomicBool) -> Result<(), String> {
    if stop.load(Ordering::SeqCst) {return Err("Audio recording was cancelled before start".into());}
    // AVFoundation's process-level grant is authoritative. In particular, a
    // CoreAudio virtual device producing samples does not establish consent.
    unsafe {
        let media=AVMediaTypeAudio.as_ref().ok_or("Microphone permission is unavailable")?;
        let status:isize=msg_send![class!(AVCaptureDevice), authorizationStatusForMediaType: media];
        match permission(status) {
            Permission::Granted => Ok(()),
            Permission::Denied => Err(DENIED.into()),
            Permission::Ask => {
                let (sender, receiver)=mpsc::channel();
                let completion=RcBlock::new(move |granted:Bool| {let _=sender.send(granted.as_bool());});
                let _:()=msg_send![class!(AVCaptureDevice), requestAccessForMediaType: media, completionHandler: &*completion];
                await_grant(receiver,stop,Duration::from_secs(60))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_explicit_authorized_status_admits_capture() {
        assert_eq!(permission(0),Permission::Ask);
        assert_eq!(permission(3),Permission::Granted);
        for status in [-1,1,2,4,isize::MAX] {assert_eq!(permission(status),Permission::Denied);}
    }
    #[test]
    fn late_permission_cannot_revive_a_stopped_recording() {
        let (tx,rx)=mpsc::channel();tx.send(true).unwrap();
        let stop=AtomicBool::new(true);
        assert!(await_grant(rx,&stop,Duration::from_secs(1)).is_err());
    }
    #[test]
    fn denial_timeout_and_lost_callback_never_admit_capture() {
        let stop=AtomicBool::new(false);
        let (tx,rx)=mpsc::channel();tx.send(false).unwrap();
        assert!(await_grant(rx,&stop,Duration::from_secs(1)).is_err());
        let (_tx,rx)=mpsc::channel();assert!(await_grant(rx,&stop,Duration::ZERO).is_err());
        let (tx,rx)=mpsc::channel();drop(tx);assert!(await_grant(rx,&stop,Duration::from_secs(1)).is_err());
        let (tx,rx)=mpsc::channel();tx.send(true).unwrap();assert!(await_grant(rx,&stop,Duration::from_secs(1)).is_ok());
    }
}
