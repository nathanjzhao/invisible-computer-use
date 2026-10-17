//! Decodes only attachment files beneath the configured Messages attachment root.
//! All parsing runs in a disposable query worker. No file path comes from read_image arguments.
use crate::{Error, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
};

const MAX_FILE: u64 = 20 * 1024 * 1024;
pub const MAX_IMAGE: usize = 2 * 1024 * 1024;
#[derive(Debug, Serialize, Deserialize)]
pub struct Image {
    pub mime_type: String,
    pub data: String,
    pub width: usize,
    pub height: usize,
}
fn refused() -> Error {
    Error::new(
        "messagesAttachmentUnavailable",
        "The attachment is missing, changed, outside the Messages attachment directory or not a regular file",
    )
}
fn unsupported() -> Error {
    Error::new(
        "messagesImageUnsupported",
        "The attachment is not a supported image or could not be decoded within its limits",
    )
}
pub fn load(database: &Path, path: &Path) -> Result<Image> {
    let root = database.parent().ok_or_else(refused)?.join("Attachments");
    let bytes = read_confined(&root, path)?;
    let (output, width, height) = decode(&bytes)?;
    Ok(Image {
        mime_type: "image/png".into(),
        data: base64::engine::general_purpose::STANDARD.encode(output),
        width,
        height,
    })
}

#[cfg(unix)]
fn read_confined(root: &Path, path: &Path) -> Result<Vec<u8>> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::{
                ffi::OsStrExt,
                fs::{MetadataExt, OpenOptionsExt},
            },
        },
    };
    let root_info = std::fs::symlink_metadata(root).map_err(|_| refused())?;
    if !root_info.is_dir() || root_info.file_type().is_symlink() {
        return Err(refused());
    }
    let canonical = root.canonicalize().map_err(|_| refused())?;
    let expanded = if let Ok(relative) = path.strip_prefix("~/Library/Messages/Attachments") {
        root.join(relative)
    } else {
        PathBuf::from(path)
    };
    let relative = expanded
        .strip_prefix(root)
        .or_else(|_| expanded.strip_prefix(&canonical))
        .map_err(|_| refused())?;
    let components = relative
        .components()
        .map(|part| {
            if let Component::Normal(name) = part {
                CString::new(name.as_bytes()).map_err(|_| refused())
            } else {
                Err(refused())
            }
        })
        .collect::<Result<Vec<_>>>()?;
    if components.is_empty() {
        return Err(refused());
    }
    let mut directory = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&canonical)
        .map_err(|_| refused())?;
    let opened = directory.metadata().map_err(|_| refused())?;
    if opened.dev() != root_info.dev() || opened.ino() != root_info.ino() {
        return Err(refused());
    }
    for (i, part) in components.iter().enumerate() {
        let last = i == components.len() - 1;
        let flags = libc::O_RDONLY
            | libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | libc::O_NONBLOCK
            | if last { 0 } else { libc::O_DIRECTORY };
        let fd = unsafe { libc::openat(directory.as_raw_fd(), part.as_ptr(), flags) };
        if fd < 0 {
            return Err(refused());
        }
        let file = unsafe { File::from_raw_fd(fd) };
        if !last {
            directory = file;
            continue;
        }
        let before = file.metadata().map_err(|_| refused())?;
        if !before.is_file() || before.len() == 0 || before.len() > MAX_FILE {
            return Err(refused());
        }
        let mut bytes = Vec::new();
        (&file)
            .take(MAX_FILE + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| refused())?;
        let after = file.metadata().map_err(|_| refused())?;
        if bytes.len() as u64 != before.len()
            || before.len() != after.len()
            || before.mtime() != after.mtime()
            || before.mtime_nsec() != after.mtime_nsec()
        {
            return Err(refused());
        }
        return Ok(bytes);
    }
    Err(refused())
}
#[cfg(not(unix))]
fn read_confined(_: &Path, _: &Path) -> Result<Vec<u8>> {
    Err(Error::new(
        "messagesPlatformUnsupported",
        "Messages attachment access requires macOS or Linux",
    ))
}

#[cfg(not(target_os = "macos"))]
fn decode(bytes: &[u8]) -> Result<(Vec<u8>, usize, usize)> {
    use image::{ImageFormat, ImageReader};
    let mut reader = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| unsupported())?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(128 * 1024 * 1024);
    limits.max_image_width = Some(30_000);
    limits.max_image_height = Some(30_000);
    reader.limits(limits);
    let image = reader.decode().map_err(|_| unsupported())?;
    for dimension in [1536, 768, 384, 192, 96] {
        let thumbnail = image.thumbnail(dimension, dimension);
        let mut output = std::io::Cursor::new(Vec::new());
        thumbnail
            .write_to(&mut output, ImageFormat::Png)
            .map_err(|_| unsupported())?;
        if output.get_ref().len() <= MAX_IMAGE {
            return Ok((
                output.into_inner(),
                thumbnail.width() as usize,
                thumbnail.height() as usize,
            ));
        }
    }
    Err(unsupported())
}

#[cfg(target_os = "macos")]
fn decode(bytes: &[u8]) -> Result<(Vec<u8>, usize, usize)> {
    native::decode(bytes)
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use core_foundation_sys::{
        base::{CFRelease, CFTypeRef},
        data::{
            CFDataCreate, CFDataCreateMutable, CFDataGetBytePtr, CFDataGetLength, CFDataRef,
            CFMutableDataRef,
        },
        dictionary::{
            CFDictionaryCreate, CFDictionaryRef, kCFTypeDictionaryKeyCallBacks,
            kCFTypeDictionaryValueCallBacks,
        },
        number::{CFNumberCreate, kCFBooleanTrue, kCFNumberSInt32Type},
        string::{
            CFStringCreateWithCString, CFStringGetCString, CFStringRef, kCFStringEncodingUTF8,
        },
    };
    use std::{
        ffi::{CStr, c_void},
        ptr,
    };
    type Source = *const c_void;
    type NativeImage = *const c_void;
    type Destination = *const c_void;
    #[link(name = "ImageIO", kind = "framework")]
    unsafe extern "C" {
        fn CGImageSourceCreateWithData(data: CFDataRef, options: CFDictionaryRef) -> Source;
        fn CGImageSourceGetType(source: Source) -> CFStringRef;
        fn CGImageSourceCreateThumbnailAtIndex(
            source: Source,
            index: usize,
            options: CFDictionaryRef,
        ) -> NativeImage;
        fn CGImageDestinationCreateWithData(
            data: CFMutableDataRef,
            kind: CFStringRef,
            count: usize,
            options: CFDictionaryRef,
        ) -> Destination;
        fn CGImageDestinationAddImage(
            destination: Destination,
            image: NativeImage,
            properties: CFDictionaryRef,
        );
        fn CGImageDestinationFinalize(destination: Destination) -> bool;
        static kCGImageSourceCreateThumbnailFromImageAlways: CFStringRef;
        static kCGImageSourceCreateThumbnailWithTransform: CFStringRef;
        static kCGImageSourceThumbnailMaxPixelSize: CFStringRef;
    }
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGImageGetWidth(image: NativeImage) -> usize;
        fn CGImageGetHeight(image: NativeImage) -> usize;
        fn CGImageRelease(image: NativeImage);
    }
    struct Owned(CFTypeRef);
    impl Owned {
        fn new(value: CFTypeRef) -> Result<Self> {
            if value.is_null() {
                Err(unsupported())
            } else {
                Ok(Self(value))
            }
        }
    }
    impl Drop for Owned {
        fn drop(&mut self) {
            unsafe { CFRelease(self.0) }
        }
    }
    struct ImageGuard(NativeImage);
    impl Drop for ImageGuard {
        fn drop(&mut self) {
            unsafe { CGImageRelease(self.0) }
        }
    }
    pub fn decode(bytes: &[u8]) -> Result<(Vec<u8>, usize, usize)> {
        unsafe {
            let data =
                Owned::new(CFDataCreate(ptr::null(), bytes.as_ptr(), bytes.len() as isize).cast())?;
            let source = Owned::new(CGImageSourceCreateWithData(data.0.cast(), ptr::null()))?;
            let kind = CGImageSourceGetType(source.0);
            let mut name = [0i8; 128];
            if kind.is_null()
                || CFStringGetCString(
                    kind,
                    name.as_mut_ptr(),
                    name.len() as isize,
                    kCFStringEncodingUTF8,
                ) == 0
            {
                return Err(unsupported());
            }
            let kind = CStr::from_ptr(name.as_ptr())
                .to_str()
                .map_err(|_| unsupported())?;
            if matches!(
                kind,
                "com.adobe.pdf" | "public.svg-image" | "com.adobe.postscript"
            ) {
                return Err(unsupported());
            }
            let png = Owned::new(
                CFStringCreateWithCString(
                    ptr::null(),
                    c"public.png".as_ptr(),
                    kCFStringEncodingUTF8,
                )
                .cast(),
            )?;
            for dimension in [1536i32, 768, 384, 192, 96] {
                let size = Owned::new(
                    CFNumberCreate(
                        ptr::null(),
                        kCFNumberSInt32Type,
                        (&dimension as *const i32).cast(),
                    )
                    .cast(),
                )?;
                let keys = [
                    kCGImageSourceCreateThumbnailFromImageAlways.cast(),
                    kCGImageSourceCreateThumbnailWithTransform.cast(),
                    kCGImageSourceThumbnailMaxPixelSize.cast(),
                ];
                let values = [kCFBooleanTrue.cast(), kCFBooleanTrue.cast(), size.0];
                let options = Owned::new(
                    CFDictionaryCreate(
                        ptr::null(),
                        keys.as_ptr(),
                        values.as_ptr(),
                        keys.len() as isize,
                        &kCFTypeDictionaryKeyCallBacks,
                        &kCFTypeDictionaryValueCallBacks,
                    )
                    .cast(),
                )?;
                let image = CGImageSourceCreateThumbnailAtIndex(source.0, 0, options.0.cast());
                if image.is_null() {
                    return Err(unsupported());
                }
                let image = ImageGuard(image);
                let width = CGImageGetWidth(image.0);
                let height = CGImageGetHeight(image.0);
                if width == 0
                    || height == 0
                    || width > dimension as usize
                    || height > dimension as usize
                {
                    return Err(unsupported());
                }
                let output = Owned::new(CFDataCreateMutable(ptr::null(), 0).cast())?;
                let encoder = Owned::new(CGImageDestinationCreateWithData(
                    output.0 as CFMutableDataRef,
                    png.0.cast(),
                    1,
                    ptr::null(),
                ))?;
                CGImageDestinationAddImage(encoder.0, image.0, ptr::null());
                if !CGImageDestinationFinalize(encoder.0) {
                    return Err(unsupported());
                }
                let length = CFDataGetLength(output.0.cast());
                if length <= 0 {
                    return Err(unsupported());
                }
                if length as usize <= MAX_IMAGE {
                    let bytes = std::slice::from_raw_parts(
                        CFDataGetBytePtr(output.0.cast()),
                        length as usize,
                    );
                    return Ok((bytes.to_vec(), width, height));
                }
            }
            Err(unsupported())
        }
    }
}
