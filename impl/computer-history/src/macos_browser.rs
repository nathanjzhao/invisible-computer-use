//! Read-only Chromium scripting properties with explicit, separate Automation consent.
use crate::{
    browser::{document_url, Page, Snapshot},
    Error, Result,
};
use std::{ffi::c_void, ptr};
const fn code(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}
// AE.framework headers pack AEDesc to two-byte alignment, including on arm64.
#[repr(C, packed(2))]
struct Desc {
    kind: u32,
    data: *mut c_void,
}
#[link(name = "CoreServices", kind = "framework")]
extern "C" {
    fn AECreateDesc(kind: u32, data: *const c_void, size: isize, out: *mut Desc) -> i16;
    fn AEDisposeDesc(desc: *mut Desc) -> i16;
    fn AECreateList(data: *const c_void, size: isize, record: u8, out: *mut Desc) -> i16;
    // AEPutKeyDesc is a header macro for this same exported function.
    fn AECoerceDesc(desc: *const Desc, kind: u32, out: *mut Desc) -> i16;
    fn AECreateAppleEvent(
        class: u32,
        event: u32,
        target: *const Desc,
        return_id: i16,
        transaction: i32,
        out: *mut Desc,
    ) -> i16;
    fn AEPutParamDesc(event: *mut Desc, key: u32, value: *const Desc) -> i16;
    fn AESendMessage(event: *const Desc, reply: *mut Desc, mode: i32, timeout: isize) -> i32;
    fn AEGetParamDesc(event: *const Desc, key: u32, kind: u32, out: *mut Desc) -> i16;
    fn AEGetDescDataSize(desc: *const Desc) -> isize;
    fn AEGetDescData(desc: *const Desc, out: *mut c_void, size: isize) -> i16;
    fn AECountItems(desc: *const Desc, count: *mut isize) -> i16;
    fn AEGetNthDesc(
        desc: *const Desc,
        index: isize,
        kind: u32,
        key: *mut u32,
        out: *mut Desc,
    ) -> i16;
    fn AEDeterminePermissionToAutomateTarget(
        target: *const Desc,
        class: u32,
        event: u32,
        ask: u8,
    ) -> i32;
}
fn failure(status: i32) -> Error {
    if matches!(status, -1743 | -1744) {
        Error::new(
            "computerHistoryBrowserPermissionRequired",
            "Allow Chrome access in History to resolve its normal and private windows",
        )
    } else {
        Error::new(
            "computerHistoryBrowserUnavailable",
            format!("Browser context could not be verified (native status {status})"),
        )
    }
}
fn okay(status: i16) -> Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(failure(i32::from(status)))
    }
}
impl Drop for Desc {
    fn drop(&mut self) {
        unsafe {
            AEDisposeDesc(self);
        }
    }
}
impl Desc {
    fn empty() -> Self {
        Self {
            kind: code(b"null"),
            data: ptr::null_mut(),
        }
    }
    fn raw(kind: u32, bytes: &[u8]) -> Result<Self> {
        let mut d = Self::empty();
        okay(unsafe { AECreateDesc(kind, bytes.as_ptr().cast(), bytes.len() as isize, &mut d) })?;
        Ok(d)
    }
    fn num(kind: u32, n: u32) -> Result<Self> {
        Self::raw(kind, &n.to_ne_bytes())
    }
    fn coerce(&self, kind: u32) -> Result<Self> {
        let mut out = Self::empty();
        okay(unsafe { AECoerceDesc(self, kind, &mut out) })?;
        Ok(out)
    }
    fn bytes(&self, limit: usize) -> Result<Vec<u8>> {
        let size = unsafe { AEGetDescDataSize(self) };
        if size < 0 || size as usize > limit {
            return Err(failure(-1700));
        }
        let mut bytes = vec![0u8; size as usize];
        okay(unsafe { AEGetDescData(self, bytes.as_mut_ptr().cast(), size) })?;
        Ok(bytes)
    }
    fn text(&self) -> Result<String> {
        let d = self.coerce(code(b"utf8"))?;
        String::from_utf8(d.bytes(65536)?).map_err(|_| failure(-1700))
    }
    fn integer(&self) -> Result<i32> {
        let d = self.coerce(code(b"long"))?;
        let b = d.bytes(4)?;
        Ok(i32::from_ne_bytes(
            b.try_into().map_err(|_| failure(-1700))?,
        ))
    }
    fn property(container: &Self, property: u32) -> Result<Self> {
        Self::object(
            code(b"prop"),
            container,
            code(b"prop"),
            &Self::num(code(b"type"), property)?,
        )
    }
    fn object(want: u32, container: &Self, form: u32, selection: &Self) -> Result<Self> {
        let mut d = Self::empty();
        okay(unsafe { AECreateList(ptr::null(), 0, 1, &mut d) })?;
        for (key, value) in [
            (code(b"want"), Self::num(code(b"type"), want)?),
            (code(b"form"), Self::num(code(b"enum"), form)?),
        ] {
            okay(unsafe { AEPutParamDesc(&mut d, key, &value) })?;
        }
        okay(unsafe { AEPutParamDesc(&mut d, code(b"from"), container) })?;
        okay(unsafe { AEPutParamDesc(&mut d, code(b"seld"), selection) })?;
        d.coerce(code(b"obj "))
    }
    fn get(&self, target: &Self) -> Result<Self> {
        let mut event = Self::empty();
        okay(unsafe {
            AECreateAppleEvent(code(b"core"), code(b"getd"), target, -1, 0, &mut event)
        })?;
        okay(unsafe { AEPutParamDesc(&mut event, code(b"----"), self) })?;
        let mut reply = Self::empty();
        // Wait at most one second. Passive sampling never prompts or activates the browser.
        let status = unsafe { AESendMessage(&event, &mut reply, 3 | 0x10 | 0x1000 | 0x20000, 60) };
        if status != 0 {
            return Err(failure(status));
        }
        let mut error = Self::empty();
        if unsafe { AEGetParamDesc(&reply, code(b"errn"), code(b"long"), &mut error) } == 0 {
            let code = error.integer()?;
            if code != 0 {
                return Err(failure(code));
            }
        }
        let mut out = Self::empty();
        okay(unsafe { AEGetParamDesc(&reply, code(b"----"), code(b"****"), &mut out) })?;
        Ok(out)
    }
    fn bounds(&self) -> Result<[i32; 4]> {
        // Chrome normally returns a QD rectangle: top, left, bottom, right, native endian.
        if self.kind == code(b"qdrt") {
            let bytes = self.bytes(8)?;
            if bytes.len() != 8 {
                return Err(failure(-1700));
            }
            let n: Vec<i32> = bytes
                .chunks_exact(2)
                .map(|b| i32::from(i16::from_ne_bytes([b[0], b[1]])))
                .collect();
            return Ok([n[1], n[0], n[3], n[2]]);
        }
        let list = self.coerce(code(b"list"))?;
        let mut count = 0isize;
        okay(unsafe { AECountItems(&list, &mut count) })?;
        if count != 4 {
            return Err(failure(-1700));
        }
        let mut out = [0i32; 4];
        for (i, v) in out.iter_mut().enumerate() {
            let mut d = Self::empty();
            let mut key = 0;
            okay(unsafe { AEGetNthDesc(&list, i as isize + 1, code(b"long"), &mut key, &mut d) })?;
            *v = d.integer()?;
        }
        Ok(out)
    }
}
/// Must run off the main thread. Called only by an explicit host action.
pub fn request_browser_permission(application: &str) -> Result<()> {
    if !crate::browser::chrome(application) {
        return Err(Error::invalid("Choose Google Chrome or Chromium"));
    }
    let target = Desc::raw(code(b"bund"), application.as_bytes())?;
    let status =
        unsafe { AEDeterminePermissionToAutomateTarget(&target, code(b"core"), code(b"getd"), 1) };
    if status == 0 {
        Ok(())
    } else {
        Err(failure(status))
    }
}
pub fn snapshot(pid: i64) -> Result<Snapshot> {
    let pid: i32 = pid.try_into().map_err(|_| failure(-600))?;
    let target = Desc::raw(code(b"kpid"), &pid.to_ne_bytes())?;
    let status =
        unsafe { AEDeterminePermissionToAutomateTarget(&target, code(b"core"), code(b"getd"), 0) };
    if status != 0 {
        return Err(failure(status));
    }
    let front = Desc::object(
        code(b"cwin"),
        &Desc::empty(),
        code(b"indx"),
        &Desc::num(code(b"long"), 1)?,
    )?;
    // Resolve the front window once. Subsequent property reads stay on that ID even
    // if an incognito window becomes frontmost between AppleEvent replies.
    let window_id_value = Desc::property(&front, code(b"ID  "))?.get(&target)?;
    let window_id = window_id_value.text()?;
    let window = Desc::object(
        code(b"cwin"),
        &Desc::empty(),
        code(b"ID  "),
        &window_id_value,
    )?;
    // Private mode is determined before asking for any page URL or tab content.
    let mode = Desc::property(&window, code(b"mode"))?
        .get(&target)?
        .text()?;
    if mode == "incognito" {
        return Ok(Snapshot::Private);
    }
    if mode != "normal" {
        return Ok(Snapshot::Other);
    }
    let bounds = Desc::property(&window, code(b"pbnd"))?
        .get(&target)?
        .bounds()?;
    let tab = Desc::property(&window, code(b"acTa"))?;
    let tab_id = Desc::property(&tab, code(b"ID  "))?.get(&target)?.text()?;
    let raw = Desc::property(&tab, code(b"URL "))?.get(&target)?.text()?;
    let Some(url) = document_url(&raw) else {
        return Ok(Snapshot::Other);
    };
    // Recheck mode and window/tab selection after the URL read.
    if Desc::property(&window, code(b"mode"))?
        .get(&target)?
        .text()?
        != "normal"
        || Desc::property(&front, code(b"ID  "))?
            .get(&target)?
            .text()?
            != window_id
        || Desc::property(&tab, code(b"ID  "))?.get(&target)?.text()? != tab_id
    {
        return Ok(Snapshot::Other);
    }
    if window_id.is_empty() || window_id.len() > 128 || tab_id.is_empty() || tab_id.len() > 128 {
        return Ok(Snapshot::Other);
    }
    Ok(Snapshot::Public(Page {
        window_id,
        tab_id,
        bounds,
        url,
    }))
}
