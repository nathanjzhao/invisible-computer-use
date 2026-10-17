mod common;
use base64::Engine;
use common::Fixture;
use icu_messages::{
    Result,
    consent::Scope,
    database::Chat,
    engine::{Host, NoHost, Output, ReadDecision},
    worker::{Query, Worker},
};
use serde_json::json;
use std::{
    io::Read,
    sync::{Arc, atomic::AtomicBool},
};
struct Allow;
impl Host for Allow {
    fn review_read(&mut self, chats: &[Chat], _: &AtomicBool) -> Result<ReadDecision> {
        Ok(ReadDecision::Accept {
            chat_guids: chats.iter().map(|c| c.chat_guid.clone()).collect(),
            scope: Scope::Session,
        })
    }
}
fn cancel() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}
fn fixture() -> Fixture {
    let f = Fixture::new();
    f.add(
        1,
        "Image fixture",
        icu_messages::dates::parse("2026-10-01T00:00:00Z").unwrap(),
    );
    f
}

#[test]
fn approved_attachment_round_trip_returns_pixels_without_source_metadata() {
    let f = fixture();
    let path = f.png();
    f.attach("image-id", &path);
    let mut engine = f.engine();
    assert_eq!(
        engine
            .call("read_image", json!({"id":"image-id"}), &mut Allow, cancel())
            .unwrap_err()
            .code,
        "messagesAttachmentNotFound"
    );
    engine
        .call(
            "read_messages",
            json!({"chat_guid":"direct"}),
            &mut Allow,
            cancel(),
        )
        .unwrap();
    let Output::Image { mime_type, data } = engine
        .call(
            "read_image",
            json!({"id":"image-id"}),
            &mut NoHost,
            cancel(),
        )
        .unwrap()
    else {
        panic!("not an image")
    };
    assert_eq!(mime_type, "image/png");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .unwrap();
    assert!(
        !bytes
            .windows(b"fixture metadata".len())
            .any(|w| w == b"fixture metadata")
    );
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().unwrap();
    let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buffer).unwrap();
    assert_eq!((info.width, info.height), (2, 1));
    assert_eq!(info.color_type, png::ColorType::Rgba);
    assert_eq!(&buffer[..4], &[255, 0, 0, 255]);
    assert_eq!(buffer[7], 128);
    assert!(buffer[6] >= 250);
}
#[test]
fn denied_reads_do_not_register_attachment_ids_and_deleted_links_cannot_be_reused() {
    let f = fixture();
    f.attach("image-id", &f.png());
    let mut engine = f.engine();
    assert!(
        engine
            .call(
                "read_messages",
                json!({"chat_guid":"direct"}),
                &mut NoHost,
                cancel()
            )
            .is_err()
    );
    assert_eq!(
        engine
            .call("read_image", json!({"id":"image-id"}), &mut Allow, cancel())
            .unwrap_err()
            .code,
        "messagesAttachmentNotFound"
    );
    engine
        .call(
            "read_messages",
            json!({"chat_guid":"direct"}),
            &mut Allow,
            cancel(),
        )
        .unwrap();
    f.writer()
        .execute("DELETE FROM message_attachment_join", [])
        .unwrap();
    assert_eq!(
        engine
            .call(
                "read_image",
                json!({"id":"image-id"}),
                &mut NoHost,
                cancel()
            )
            .unwrap_err()
            .code,
        "messagesAttachmentNotFound"
    );
}
#[test]
fn worker_refuses_path_escape_symlinks_and_non_regular_files() {
    use std::os::unix::fs::symlink;
    let f = fixture();
    let png = f.png();
    let outside = f.dir.path().join("outside.png");
    std::fs::copy(&png, &outside).unwrap();
    let before = std::fs::read(&outside).unwrap();
    let symlink_file = f.dir.path().join("Attachments/link.png");
    symlink(&outside, &symlink_file).unwrap();
    let symlink_dir = f.dir.path().join("Attachments/dir-link");
    symlink(f.dir.path(), &symlink_dir).unwrap();
    let fifo = f.dir.path().join("Attachments/pipe.png");
    let c_path = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
    let paths = [
        outside.clone(),
        f.dir.path().join("Attachments/../outside.png"),
        symlink_file,
        symlink_dir.join("outside.png"),
        fifo,
        f.dir.path().join("Attachments"),
    ];
    let worker = Worker::new(
        env!("CARGO_BIN_EXE_icu-messages").into(),
        f.dir.path().join("chat.db"),
    )
    .unwrap();
    for (i, path) in paths.iter().enumerate() {
        let id = format!("bad-{i}");
        f.attach(&id, path);
        let error = worker
            .query(
                Query::Image {
                    chat_guid: "direct".into(),
                    message_guid: format!(
                        "m-{}",
                        icu_messages::dates::parse("2026-10-01T00:00:00Z").unwrap()
                    ),
                    attachment_id: id,
                },
                cancel(),
            )
            .unwrap_err();
        assert_eq!(
            error.code,
            "messagesAttachmentUnavailable",
            "{}: {}",
            path.display(),
            error.message
        );
    }
    assert_eq!(std::fs::read(outside).unwrap(), before);
}
#[test]
fn malformed_and_oversized_images_fail_without_breaking_subsequent_reads() {
    let f = fixture();
    let good = f.png();
    let malformed = f.dir.path().join("Attachments/bad.png");
    std::fs::write(&malformed, b"not an image").unwrap();
    f.attach("bad", &malformed);
    let oversized = f.dir.path().join("Attachments/large.png");
    std::fs::File::create(&oversized)
        .unwrap()
        .set_len(21 * 1024 * 1024)
        .unwrap();
    f.attach("large", &oversized);
    f.attach("good", &good);
    let mut engine = f.engine();
    engine
        .call(
            "read_messages",
            json!({"chat_guid":"direct"}),
            &mut Allow,
            cancel(),
        )
        .unwrap();
    assert_eq!(
        engine
            .call("read_image", json!({"id":"bad"}), &mut NoHost, cancel())
            .unwrap_err()
            .code,
        "messagesImageUnsupported"
    );
    assert_eq!(
        engine
            .call("read_image", json!({"id":"large"}), &mut NoHost, cancel())
            .unwrap_err()
            .code,
        "messagesAttachmentUnavailable"
    );
    assert!(matches!(
        engine
            .call("read_image", json!({"id":"good"}), &mut NoHost, cancel())
            .unwrap(),
        Output::Image { .. }
    ));
    let mut unchanged = Vec::new();
    std::fs::File::open(malformed)
        .unwrap()
        .read_to_end(&mut unchanged)
        .unwrap();
    assert_eq!(unchanged, b"not an image");
}

#[test]
fn every_attachment_reference_in_a_large_fresh_page_remains_usable() {
    use rusqlite::params;
    let f = fixture();
    let path = f.png();
    let d = icu_messages::dates::parse("2026-10-01T00:00:00Z").unwrap();
    f.add(1, "second", d + 1_000_000_000);
    f.add(1, "third", d + 2_000_000_000);
    let mut c = f.writer();
    let tx = c.transaction().unwrap();
    for i in 0..1001 {
        tx.execute(
            "INSERT INTO attachment VALUES(?,?, 'image/png',NULL,'fixture.png')",
            params![format!("many-{i}"), path.to_str().unwrap()],
        )
        .unwrap();
        let aid = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO message_attachment_join VALUES(?,?)",
            params![i / 500 + 1, aid],
        )
        .unwrap();
    }
    tx.commit().unwrap();
    let mut engine = f.engine();
    let Output::Json(page) = engine
        .call(
            "read_messages",
            json!({"chat_guid":"direct"}),
            &mut Allow,
            cancel(),
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(
        page["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["attachments"].as_array().unwrap().len())
            .sum::<usize>(),
        1001
    );
    assert!(matches!(
        engine
            .call(
                "read_image",
                json!({"id":"many-1000"}),
                &mut NoHost,
                cancel()
            )
            .unwrap(),
        Output::Image { .. }
    ));
}

#[test]
#[cfg(target_os = "macos")]
fn native_heic_and_exif_orientation_are_decoded_in_workers() {
    let f = fixture();
    std::fs::create_dir_all(f.dir.path().join("Attachments")).unwrap();
    let source = f.dir.path().join("Attachments/source.png");
    let mut encoder = png::Encoder::new(std::fs::File::create(&source).unwrap(), 128, 64);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer
        .write_image_data(&[220, 40, 30].repeat(128 * 64))
        .unwrap();
    writer.finish().unwrap();
    for format in ["heic", "jpeg"] {
        let path = f.dir.path().join(format!("Attachments/source.{format}"));
        let result = std::process::Command::new("/usr/bin/sips")
            .args(["-s", "format", format])
            .arg(&source)
            .arg("--out")
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        if format == "jpeg" {
            let original = std::fs::read(&path).unwrap();
            assert_eq!(&original[..2], &[0xff, 0xd8]);
            // Independent EXIF fixture: little-endian TIFF with orientation 6 (rotate right).
            let exif =
                b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
            let mut oriented = vec![0xff, 0xd8, 0xff, 0xe1];
            oriented.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
            oriented.extend_from_slice(exif);
            oriented.extend_from_slice(&original[2..]);
            std::fs::write(&path, oriented).unwrap();
        }
        f.attach(format, &path);
    }
    let mut engine = f.engine();
    engine
        .call(
            "read_messages",
            json!({"chat_guid":"direct"}),
            &mut Allow,
            cancel(),
        )
        .unwrap();
    for (id, dimensions) in [("heic", (128, 64)), ("jpeg", (64, 128))] {
        let Output::Image { data, .. } = engine
            .call("read_image", json!({"id":id}), &mut NoHost, cancel())
            .unwrap()
        else {
            panic!("not an image")
        };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let reader = decoder.read_info().unwrap();
        assert_eq!((reader.info().width, reader.info().height), dimensions);
    }
}
