//! Local conversion of completed audio. This never starts a recorder or a sensor.
use std::{collections::BTreeSet, fs::{self, File, OpenOptions}, io::{BufReader, BufWriter, Seek, SeekFrom, Write}, path::{Path, PathBuf}, time::{Duration, Instant}};
use serde::Serialize;
use serde_json::Value;

const RATE: u32 = 24_000;
const CHANNELS: u16 = 2;
const MAX_INPUT: u64 = 128 * 1024 * 1024;
const MAX_FRAMES: u64 = RATE as u64 * 3602;

#[derive(Debug, Serialize)]
pub struct Export {
    pub directory: String,
    pub files: Vec<ExportedFile>,
}
#[derive(Debug, Serialize)]
pub struct ExportedFile {
    pub source: String,
    pub filepath: String,
    pub mime: &'static str,
    pub sample_rate: u32,
    pub channels: u16,
    pub byte_count: u64,
    pub duration_seconds: f64,
}

fn error(e: impl std::fmt::Display) -> String { format!("WAV export: {e}") }
fn shown(path: &Path) -> String { crate::audio::display_path(&path.to_string_lossy()) }

/// The caller supplies the app-owned state snapshot, never a user-selected path.
pub fn export(root: &Path, status: &Value) -> Result<Export, String> {
    if !matches!(status["state"].as_str(), Some("saved" | "partial")) {
        return Err("Finish a recording before exporting WAV files".into());
    }
    let root = root.canonicalize().map_err(error)?;
    let directory = Path::new(status["directory"].as_str().ok_or("Audio recording folder is unavailable")?)
        .canonicalize().map_err(error)?;
    if directory.parent() != Some(root.as_path()) { return Err("Audio recording is outside its storage folder".into()); }
    let sources = status["sources"].as_array().ok_or("Audio sources are unavailable")?;
    if sources.is_empty() || sources.len() > 2 { return Err("No completed audio sources to export".into()); }
    let mut names = BTreeSet::new();
    let mut inputs = Vec::new();
    for source in sources {
        let name = source["name"].as_str().ok_or("Audio source has no name")?;
        if !matches!(name, "mic" | "system") || !names.insert(name.to_owned()) { return Err("Audio source is invalid or repeated".into()); }
        let duration = source["duration_sec"].as_f64().filter(|n| n.is_finite() && *n >= 0. && *n <= 3602.)
            .ok_or("Audio source has an invalid duration")?;
        // Partial captures can retain a zero-sample entry for an unavailable
        // source. Export the successful sources without inventing silent audio.
        if duration == 0. { continue; }
        let filename = format!("audio-{name}.opus");
        if source["file"].as_str() != Some(filename.as_str()) { return Err("Audio source filename is invalid".into()); }
        let path = directory.join(&filename).canonicalize().map_err(error)?;
        if path.parent() != Some(directory.as_path()) { return Err("Audio source is outside its recording folder".into()); }
        let mut options = OpenOptions::new(); options.read(true);
        #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK); }
        #[cfg(windows)] { use std::os::windows::fs::OpenOptionsExt; options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT); }
        let file = options.open(&path).map_err(error)?;
        let metadata = file.metadata().map_err(error)?;
        #[cfg(windows)] {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err("Audio source changed to a link".into());
            }
        }
        if !metadata.is_file() || metadata.len() > MAX_INPUT { return Err("Audio source is not a supported recording file".into()); }
        inputs.push((name.to_owned(), file));
    }
    if inputs.is_empty() { return Err("No completed audio sources to export".into()); }
    // Reserve a new directory atomically. Existing exports and user files stay intact.
    let out = reserve_directory(&directory)?;
    let result = (|| {
        let mut files = Vec::new();
        for (source, input) in inputs {
            let path = out.join(format!("audio-{source}.wav"));
            let frames = convert(input, &path)?;
            files.push(ExportedFile { source, filepath: shown(&path), mime: "audio/wav", sample_rate: RATE,
                channels: CHANNELS, byte_count: 44 + frames * 4, duration_seconds: frames as f64 / RATE as f64 });
        }
        Ok(Export { directory: shown(&out), files })
    })();
    if result.is_err() { let _ = fs::remove_dir_all(&out); }
    result
}

fn reserve_directory(recording: &Path) -> Result<PathBuf, String> {
    for index in 1..=100 {
        let path = recording.join(if index == 1 { "wav".into() } else { format!("wav-{index}") });
        match fs::create_dir(&path) {
            Ok(()) => {
                #[cfg(unix)] {
                    use std::os::unix::fs::PermissionsExt;
                    if let Err(e) = fs::set_permissions(&path, fs::Permissions::from_mode(0o700)) {
                        let _ = fs::remove_dir(&path); return Err(error(e));
                    }
                }
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {},
            Err(e) => return Err(error(e)),
        }
    }
    Err("This recording already has 100 WAV exports".into())
}

fn header(output: &mut BufWriter<File>, frames: u64) -> Result<(), String> {
    let bytes = u32::try_from(frames.checked_mul(4).ok_or("Audio is too long")?).map_err(error)?;
    output.seek(SeekFrom::Start(0)).map_err(error)?;
    let mut h = Vec::with_capacity(44);
    h.extend_from_slice(b"RIFF"); h.extend_from_slice(&(bytes + 36).to_le_bytes()); h.extend_from_slice(b"WAVEfmt ");
    h.extend_from_slice(&16u32.to_le_bytes()); h.extend_from_slice(&1u16.to_le_bytes()); h.extend_from_slice(&CHANNELS.to_le_bytes());
    h.extend_from_slice(&RATE.to_le_bytes()); h.extend_from_slice(&(RATE * 4).to_le_bytes());
    h.extend_from_slice(&4u16.to_le_bytes()); h.extend_from_slice(&16u16.to_le_bytes());
    h.extend_from_slice(b"data"); h.extend_from_slice(&bytes.to_le_bytes());
    output.write_all(&h).map_err(error)
}

fn convert(input: File, target: &Path) -> Result<u64, String> {
    let mut packets = ogg::PacketReader::new(BufReader::new(input));
    let head = packets.read_packet().map_err(error)?.ok_or("Empty audio recording")?;
    if !head.first_in_stream() || head.data.len() != 19 || &head.data[..8] != b"OpusHead"
        || head.data[8] != 1 || !matches!(head.data[9], 1 | 2) || head.data[18] != 0 {
        return Err("Audio recording has an unsupported Opus header".into());
    }
    let serial = head.stream_serial();
    let preskip = u64::from(u16::from_le_bytes([head.data[10], head.data[11]]));
    if preskip % 2 != 0 { return Err("Audio recording has an unsupported sample offset".into()); }
    let skip = preskip / 2;
    let tags = packets.read_packet().map_err(error)?.ok_or("Audio recording has no Opus tags")?;
    if tags.stream_serial() != serial || tags.data.len() > 65536 || !tags.data.starts_with(b"OpusTags") || tags.last_in_stream() {
        return Err("Audio recording has invalid Opus tags".into());
    }
    // libopus resamples to 24 kHz and widens mono to stereo. Ogg granules remain
    // in 48 kHz units, so both the initial skip and final trim are scaled below.
    let mut decoder = opus::Decoder::new(RATE, opus::Channels::Stereo).map_err(error)?;
    decoder.set_gain(i32::from(i16::from_le_bytes([head.data[16], head.data[17]]))).map_err(error)?;
    let mut options = OpenOptions::new(); options.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    let temporary = target.with_file_name(format!(".{}.part", target.file_name().and_then(|n|n.to_str()).ok_or("Invalid WAV filename")?));
    let mut output = BufWriter::new(options.open(&temporary).map_err(error)?);
    header(&mut output, 0)?;
    let mut buffer = [0i16; 5760]; // 120 ms, stereo, 24 kHz.
    let (mut decoded, mut written, mut ended) = (0u64, 0u64, false);
    let deadline = Instant::now() + Duration::from_secs(120);
    while let Some(packet) = packets.read_packet().map_err(error)? {
        if Instant::now() >= deadline { return Err("WAV export took too long".into()); }
        if ended || packet.stream_serial() != serial || packet.first_in_stream() || packet.data.is_empty() || packet.data.len() > 65536 {
            return Err("Audio recording contains an invalid or additional stream".into());
        }
        let count = decoder.decode(&packet.data, &mut buffer, false).map_err(error)? as u64;
        let mut end = decoded.checked_add(count).ok_or("Audio sample count overflow")?;
        if end > MAX_FRAMES + skip { return Err("Audio recording exceeds one hour".into()); }
        if packet.last_in_stream() {
            let granule = packet.absgp_page();
            if granule > end * 2 || granule < decoded * 2 { return Err("Audio recording has an invalid final sample count".into()); }
            // A manual stop can leave an odd count at 48 kHz. Preserve its last
            // half-frame when converting to 24 kHz, as an independent decoder
            // does. Millisecond capture limits always have an even granule.
            end = granule.div_ceil(2); ended = true;
        }
        let begin = decoded.max(skip);
        if end > begin {
            let start = ((begin - decoded) * 2) as usize;
            let finish = ((end - decoded) * 2) as usize;
            for sample in &buffer[start..finish] { output.write_all(&sample.to_le_bytes()).map_err(error)?; }
            written += end - begin;
        }
        decoded += count;
    }
    if !ended || written == 0 { return Err("Audio recording is incomplete or has no samples".into()); }
    header(&mut output, written)?;
    output.flush().map_err(error)?; output.get_ref().sync_all().map_err(error)?;
    drop(output);
    // The operation owns its newly reserved private directory. Publish each
    // WAV only after its sample count and durable header are complete.
    fs::rename(temporary, target).map_err(error)?;
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const FIXTURE: &[u8] = include_bytes!("../tests/fixtures/audio-mic.opus");
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!("icu-wav-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
            fs::create_dir(&path).unwrap();fs::create_dir(path.join("recording")).unwrap();
            fs::write(path.join("recording/audio-mic.opus"),FIXTURE).unwrap();Self(path)
        }
        fn status(&self) -> Value {json!({"state":"saved","directory":self.0.join("recording"),"sources":[{"name":"mic","file":"audio-mic.opus","duration_sec":1.86}]})}
    }
    impl Drop for Fixture {fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
    #[test]
    fn native_recording_becomes_stereo_pcm_with_exact_trim() {
        let f=Fixture::new();let r=export(&f.0,&f.status()).unwrap();let data=fs::read(&r.files[0].filepath).unwrap();
        assert_eq!(&data[..4],b"RIFF");assert_eq!(&data[8..12],b"WAVE");
        assert_eq!(u16::from_le_bytes(data[20..22].try_into().unwrap()),1);
        assert_eq!(u16::from_le_bytes(data[22..24].try_into().unwrap()),2);
        assert_eq!(u32::from_le_bytes(data[24..28].try_into().unwrap()),24000);
        // Independent FFmpeg decode of the native fixture: 1.8535 seconds.
        assert_eq!(data.len(),44+44484*4);assert_eq!(r.files[0].byte_count,data.len() as u64);
        let pcm:Vec<f64>=data[44..].chunks_exact(4).map(|s|i16::from_le_bytes([s[0],s[1]]) as f64/32768.).collect();
        let window=&pcm[12000..24000];
        let amplitude=|hz:f64|{let (a,b)=window.iter().enumerate().fold((0.,0.),|(a,b),(i,x)|{let phase=2.*std::f64::consts::PI*hz*i as f64/24000.;(a+x*phase.cos(),b+x*phase.sin())});2.*a.hypot(b)/window.len() as f64};
        assert!(amplitude(880.)>0.15);assert!(amplitude(440.)<0.01);
        let second=export(&f.0,&f.status()).unwrap();assert_ne!(r.directory,second.directory);assert_eq!(data,fs::read(&r.files[0].filepath).unwrap());
    }
    #[test]
    fn odd_final_sample_count_exports_the_last_output_frame() {
        // Signed Linux capture stopped at 206307 frames at 48 kHz. Independent
        // FFmpeg decoding produces 103154 stereo frames at 24 kHz.
        let f=Fixture::new();
        fs::write(f.0.join("recording/audio-mic.opus"),include_bytes!("../tests/fixtures/audio-mic-odd-final.opus")).unwrap();
        let mut status=f.status();status["sources"][0]["duration_sec"]=json!(206307.0/48000.0);
        let result=export(&f.0,&status).unwrap();
        let data=fs::read(&result.files[0].filepath).unwrap();
        assert_eq!(data.len(),44+103154*4);
        assert_eq!(u32::from_le_bytes(data[40..44].try_into().unwrap()),103154*4);
        assert_eq!(result.files[0].duration_seconds,103154.0/24000.0);
    }
    #[test]
    fn malformed_or_unfinished_data_never_leaves_an_export() {
        for data in [vec![0;32],FIXTURE[..FIXTURE.len()-50].to_vec()] {
            let f=Fixture::new();fs::write(f.0.join("recording/audio-mic.opus"),data).unwrap();
            assert!(export(&f.0,&f.status()).is_err());assert!(!f.0.join("recording/wav").exists());
        }
    }
    #[test]
    fn only_completed_owned_sources_can_be_exported() {
        let f=Fixture::new();let mut s=f.status();s["state"]=json!("recording");assert!(export(&f.0,&s).is_err());
        s=f.status();s["sources"][0]["file"]=json!("../audio-mic.opus");assert!(export(&f.0,&s).is_err());
        s=f.status();s["sources"][0]["duration_sec"]=json!(0);assert!(export(&f.0,&s).is_err());
        s=f.status();s["sources"]=json!([s["sources"][0],s["sources"][0]]);assert!(export(&f.0,&s).is_err());
        assert!(!f.0.join("recording/wav").exists());
    }
    #[test]
    fn a_partial_capture_exports_only_sources_with_samples() {
        let f=Fixture::new();let mut s=f.status();s["state"]=json!("partial");
        s["sources"].as_array_mut().unwrap().push(json!({"name":"system","file":"audio-system.opus","duration_sec":0}));
        let r=export(&f.0,&s).unwrap();assert_eq!(r.files.len(),1);assert_eq!(r.files[0].source,"mic");
        assert!(!Path::new(&r.directory).join("audio-system.wav").exists());
    }
    #[cfg(unix)]
    #[test]
    fn a_source_symlink_cannot_read_outside_its_recording() {
        let f=Fixture::new();let source=f.0.join("recording/audio-mic.opus");fs::remove_file(&source).unwrap();
        let outside=f.0.join("outside.opus");fs::write(&outside,FIXTURE).unwrap();
        std::os::unix::fs::symlink(outside,source).unwrap();assert!(export(&f.0,&f.status()).is_err());
    }
}
