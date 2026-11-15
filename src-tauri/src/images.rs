//! Cover image pipeline: saves album art as JPEG and pre-generates a tiny
//! blurred variant used as the Now Playing background (avoids runtime
//! backdrop-filter, the folia perf killer).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use image::{codecs::jpeg::JpegEncoder, DynamicImage, ExtendedColorType, ImageEncoder as _};

// Only publication needs serializing: encoding happens in independent temp files.
// A single short-lived lock also keeps each main/blur pair from different writers
// from interleaving, without retaining a lock entry for every album ever seen.
static COVER_PUBLICATION: Mutex<()> = Mutex::new(());
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn cover_paths(covers_dir: &Path, album_key: &str) -> (PathBuf, PathBuf) {
    let mut h = DefaultHasher::new();
    album_key.hash(&mut h);
    let name = format!("{:016x}", h.finish());
    (
        covers_dir.join(format!("{name}.jpg")),
        covers_dir.join(format!("{name}.blur.jpg")),
    )
}

/// Blur path matching a main cover path by naming convention.
pub fn blur_of(main: &Path) -> PathBuf {
    let stem = main.file_stem().and_then(|s| s.to_str()).unwrap_or("cover");
    main.with_file_name(format!("{stem}.blur.jpg"))
}

/// A cache-busting version, read when constructing a cover DTO, not on each tick.
pub fn file_version(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(format!("{}-{}", modified.as_nanos(), metadata.len()))
}

/// Owns an unpublished, same-directory file and removes it on every error path.
struct PendingJpeg {
    path: PathBuf,
    published: bool,
}

impl PendingJpeg {
    fn create(target: &Path) -> Option<(Self, std::fs::File)> {
        loop {
            let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let name = format!(
                ".{}.{}.{sequence}.tmp",
                target.file_name()?.to_string_lossy(),
                std::process::id(),
            );
            let path = target.with_file_name(name);
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => {
                    return Some((
                        Self {
                            path,
                            published: false,
                        },
                        file,
                    ))
                }
                // A previous process with the same PID may have left a file.
                // Never truncate or remove a temp file that we do not own.
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return None,
            }
        }
    }

    fn publish(mut self, target: &Path) -> Option<()> {
        // Do not remove the old target first: replacement must be one rename.
        std::fs::rename(&self.path, target).ok()?;
        self.published = true;
        Some(())
    }
}

impl Drop for PendingJpeg {
    fn drop(&mut self) {
        if !self.published {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn encode_jpeg(path: &Path, img: &DynamicImage, quality: u8) -> Option<PendingJpeg> {
    let (pending, mut file) = PendingJpeg::create(path)?;
    let encoded = JpegEncoder::new_with_quality(&mut file, quality)
        .write_image(
            img.as_bytes(),
            img.width(),
            img.height(),
            ExtendedColorType::Rgb8,
        )
        .is_ok();
    let flushed = encoded && file.flush().is_ok();
    // Close before either publishing or cleaning up (also necessary on Windows).
    drop(file);
    if flushed {
        Some(pending)
    } else {
        None
    }
}

/// Save cover bytes as `{hash}.jpg` plus a pre-blurred `{hash}.blur.jpg`.
/// `key` is the album key, or a synthetic `track:{id}` for per-track covers.
pub fn save_cover(covers_dir: &Path, key: &str, bytes: &[u8]) -> Option<PathBuf> {
    let img = image::load_from_memory(bytes).ok()?;
    let rgb = DynamicImage::ImageRgb8(img.to_rgb8());
    std::fs::create_dir_all(covers_dir).ok()?;
    let (main, blur) = cover_paths(covers_dir, key);
    let small = rgb.thumbnail(48, 48);
    let blurred = small.blur(6.0);

    publish_cover(&main, &blur, &rgb, &blurred)?;
    Some(main)
}

fn publish_cover(
    main: &Path,
    blur: &Path,
    rgb: &DynamicImage,
    blurred: &DynamicImage,
) -> Option<()> {
    // Finish BOTH encodes before changing either visible path. Decode/encode
    // failures therefore preserve the previous pair, and all temps are cleaned.
    let pending_main = encode_jpeg(main, rgb, 88)?;
    let pending_blur = encode_jpeg(blur, blurred, 80)?;
    let _publication = COVER_PUBLICATION.lock().ok()?;
    // Two filenames cannot form one atomic transaction. Publish main last so a
    // successful main/version update always has its matching blur already ready.
    // Readers may briefly see old main/new blur, but never a partly written JPEG.
    // If the second rename fails, main stays intact although blur may be newer.
    pending_blur.publish(blur)?;
    pending_main.publish(main)?;
    Some(())
}

/// Per-track cover: shadows the album cover for exactly one track.
pub fn save_track_cover(covers_dir: &Path, track_id: i64, bytes: &[u8]) -> Option<PathBuf> {
    save_cover(covers_dir, &format!("track:{track_id}"), bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::{BufReader, Cursor};
    use std::sync::mpsc;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            loop {
                let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
                let path = std::env::temp_dir().join(format!(
                    "sonora-cover-test-{}-{sequence}",
                    std::process::id(),
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("create test directory: {error}"),
                }
            }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            // This directory was exclusively created by this test, never reused.
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn frame(seed: u8) -> DynamicImage {
        DynamicImage::ImageRgb8(image::RgbImage::from_fn(96, 80, |x, y| {
            let seed = u32::from(seed);
            image::Rgb([
                ((x * 17 + y * 3 + seed * 43) % 256) as u8,
                ((x * 5 + y * 19 + seed * 71) % 256) as u8,
                ((x * 11 + y * 7 + seed * 97) % 256) as u8,
            ])
        }))
    }

    fn png(img: &DynamicImage) -> Vec<u8> {
        let mut output = Cursor::new(Vec::new());
        img.write_to(&mut output, image::ImageFormat::Png).unwrap();
        output.into_inner()
    }

    fn jpeg(img: &DynamicImage, quality: u8) -> Vec<u8> {
        let mut output = Vec::new();
        JpegEncoder::new_with_quality(&mut output, quality)
            .write_image(
                img.as_bytes(),
                img.width(),
                img.height(),
                ExtendedColorType::Rgb8,
            )
            .unwrap();
        output
    }

    fn assert_no_temps(dir: &Path) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            assert_ne!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("tmp"),
                "unpublished temp remains: {path:?}"
            );
        }
    }

    fn assert_complete(path: &Path, expected: &[Vec<u8>]) {
        let bytes = fs::read(path).unwrap();
        // JPEG decoders may tolerate truncation; exact membership also checks
        // every byte, including the final scanlines and end-of-image marker.
        assert!(
            expected.contains(&bytes),
            "not a complete published JPEG: {path:?}"
        );
        let decoded = image::load_from_memory(&bytes).unwrap().to_rgb8();
        assert!(decoded.width() > 0 && decoded.height() > 0);
    }

    #[test]
    fn saves_complete_main_and_matching_blur_without_temps() {
        let dir = TestDir::new();
        let img = frame(1);
        let main = save_cover(&dir.0, "album", &png(&img)).unwrap();
        let blur = blur_of(&main);
        assert_eq!((main.clone(), blur.clone()), cover_paths(&dir.0, "album"));
        assert_eq!(fs::read(&main).unwrap(), jpeg(&img, 88));
        assert_eq!(
            fs::read(&blur).unwrap(),
            jpeg(&img.thumbnail(48, 48).blur(6.0), 80)
        );
        assert_eq!(image::open(&main).unwrap().to_rgb8().dimensions(), (96, 80));
        assert_eq!(image::open(&blur).unwrap().to_rgb8().dimensions(), (48, 40));
        assert_no_temps(&dir.0);
    }

    #[test]
    fn decode_failure_preserves_the_previous_pair() {
        let dir = TestDir::new();
        let main = save_cover(&dir.0, "album", &png(&frame(1))).unwrap();
        let blur = blur_of(&main);
        let old_main = fs::read(&main).unwrap();
        let old_blur = fs::read(&blur).unwrap();
        assert!(save_cover(&dir.0, "album", b"not an image").is_none());
        assert_eq!(fs::read(&main).unwrap(), old_main);
        assert_eq!(fs::read(&blur).unwrap(), old_blur);
        assert_no_temps(&dir.0);
    }

    #[test]
    fn jpeg_encode_failure_preserves_the_previous_pair_and_cleans_temp() {
        let dir = TestDir::new();
        let main = save_cover(&dir.0, "album", &png(&frame(1))).unwrap();
        let blur = blur_of(&main);
        let old_main = fs::read(&main).unwrap();
        let old_blur = fs::read(&blur).unwrap();
        // Valid PNG, but JPEG's dimensions are limited to u16. This reaches
        // the real save_cover encode failure path without permissions or sleeps.
        let too_wide = DynamicImage::new_rgb8(65_536, 1);
        assert!(save_cover(&dir.0, "album", &png(&too_wide)).is_none());
        assert_eq!(fs::read(&main).unwrap(), old_main);
        assert_eq!(fs::read(&blur).unwrap(), old_blur);
        assert_no_temps(&dir.0);
    }

    #[test]
    fn second_encode_failure_preserves_both_files_and_cleans_staged_main() {
        let dir = TestDir::new();
        let main = save_cover(&dir.0, "album", &png(&frame(1))).unwrap();
        let blur = blur_of(&main);
        let old_main = fs::read(&main).unwrap();
        let old_blur = fs::read(&blur).unwrap();
        let invalid_blur = DynamicImage::new_rgb8(0, 0);
        assert!(publish_cover(&main, &blur, &frame(2), &invalid_blur).is_none());
        assert_eq!(fs::read(&main).unwrap(), old_main);
        assert_eq!(fs::read(&blur).unwrap(), old_blur);
        assert_no_temps(&dir.0);
    }

    #[test]
    fn failed_blur_rename_preserves_main_and_cleans_both_temps() {
        let dir = TestDir::new();
        let main = save_cover(&dir.0, "album", &png(&frame(1))).unwrap();
        let old_main = fs::read(&main).unwrap();
        let blocked_blur = dir.0.join("blocked-blur.jpg");
        fs::create_dir(&blocked_blur).unwrap();
        let img = frame(2);
        let blurred = img.thumbnail(48, 48).blur(6.0);
        assert!(publish_cover(&main, &blocked_blur, &img, &blurred).is_none());
        assert_eq!(fs::read(&main).unwrap(), old_main);
        assert!(blocked_blur.is_dir());
        assert_no_temps(&dir.0);
    }

    #[test]
    fn failed_main_rename_keeps_target_and_published_blur_and_cleans_temp() {
        let dir = TestDir::new();
        let main = save_cover(&dir.0, "album", &png(&frame(1))).unwrap();
        let blur = blur_of(&main);
        let blocked_main = dir.0.join("blocked-main.jpg");
        fs::create_dir(&blocked_main).unwrap();
        let old_main = fs::read(&main).unwrap();
        let img = frame(2);
        let blurred = img.thumbnail(48, 48).blur(6.0);
        // A directory deterministically rejects the second rename. There is no
        // delete-first fallback; the existing target survives and blur is whole.
        assert!(publish_cover(&blocked_main, &blur, &img, &blurred).is_none());
        assert!(blocked_main.is_dir());
        assert_eq!(fs::read(&main).unwrap(), old_main);
        assert_complete(&blur, &[jpeg(&blurred, 80)]);
        assert_no_temps(&dir.0);
    }

    #[test]
    fn open_readers_keep_the_entire_old_pair_across_publication() {
        let dir = TestDir::new();
        let main = save_cover(&dir.0, "album", &png(&frame(1))).unwrap();
        let blur = blur_of(&main);
        let expected_main = image::open(&main).unwrap().to_rgb8();
        let expected_blur = image::open(&blur).unwrap().to_rgb8();
        let old_main = File::open(&main).unwrap();
        let old_blur = File::open(&blur).unwrap();
        assert_eq!(save_cover(&dir.0, "album", &png(&frame(2))).unwrap(), main);
        // Decode only AFTER replacement: in-place truncation would have changed
        // these already-open files too. Atomic rename leaves their inodes alone.
        let decoded_main = image::load(BufReader::new(old_main), image::ImageFormat::Jpeg)
            .unwrap()
            .to_rgb8();
        let decoded_blur = image::load(BufReader::new(old_blur), image::ImageFormat::Jpeg)
            .unwrap()
            .to_rgb8();
        assert_eq!(decoded_main, expected_main);
        assert_eq!(decoded_blur, expected_blur);
        assert_ne!(image::open(&main).unwrap().to_rgb8(), expected_main);
        assert_no_temps(&dir.0);
    }

    #[test]
    fn concurrent_repeated_publication_always_reads_complete_images() {
        const WRITERS: usize = 3;
        const PUBLICATIONS: usize = 16;
        let dir = TestDir::new();
        let frames: Vec<_> = (0..WRITERS).map(|seed| frame(seed as u8)).collect();
        let inputs: Vec<_> = frames.iter().map(png).collect();
        let expected_main: Vec<_> = frames.iter().map(|img| jpeg(img, 88)).collect();
        let expected_blur: Vec<_> = frames
            .iter()
            .map(|img| jpeg(&img.thumbnail(48, 48).blur(6.0), 80))
            .collect();
        let main = save_cover(&dir.0, "album", &inputs[0]).unwrap();
        let blur = blur_of(&main);
        std::thread::scope(|scope| {
            let writers: Vec<_> = inputs
                .iter()
                .map(|input| {
                    let covers_dir = &dir.0;
                    let main = &main;
                    let (request, requests) = mpsc::channel::<()>();
                    let (finished, completion) = mpsc::channel();
                    let worker = scope.spawn(move || {
                        for _ in 0..PUBLICATIONS {
                            if requests.recv().is_err() {
                                return;
                            }
                            let published = save_cover(covers_dir, "album", input);
                            if finished.send(published.as_ref() == Some(main)).is_err() {
                                return;
                            }
                        }
                    });
                    (request, completion, worker)
                })
                .collect();
            for _ in 0..PUBLICATIONS {
                // Workers are parked until requested. Hold old readers across
                // each concurrent batch, then fully decode them after all writes.
                // This forces the old-reader/new-publication overlap without
                // relying on sleeps or the scheduler catching a partial write.
                let old_main = File::open(&main).unwrap();
                let old_blur = File::open(&blur).unwrap();
                let previous_main = image::open(&main).unwrap().to_rgb8();
                let previous_blur = image::open(&blur).unwrap().to_rgb8();
                for (request, _, _) in &writers {
                    request.send(()).unwrap();
                }
                for _ in 0..4 {
                    assert_complete(&main, &expected_main);
                    assert_complete(&blur, &expected_blur);
                }
                for (_, completion, _) in &writers {
                    assert!(completion.recv().unwrap());
                }
                assert_eq!(
                    image::load(BufReader::new(old_main), image::ImageFormat::Jpeg)
                        .unwrap()
                        .to_rgb8(),
                    previous_main,
                );
                assert_eq!(
                    image::load(BufReader::new(old_blur), image::ImageFormat::Jpeg)
                        .unwrap()
                        .to_rgb8(),
                    previous_blur,
                );
            }
            for (_, _, writer) in writers {
                writer.join().unwrap();
            }
        });
        // Once all publishers finish, serialization must leave a matching pair.
        let final_main = fs::read(&main).unwrap();
        let generation = expected_main
            .iter()
            .position(|bytes| bytes == &final_main)
            .unwrap();
        assert_eq!(fs::read(&blur).unwrap(), expected_blur[generation]);
        assert_no_temps(&dir.0);
    }

    #[test]
    fn file_version_changes_on_atomic_overwrite_at_the_same_path() {
        let dir = TestDir::new();
        let img = frame(1);
        let main = save_cover(&dir.0, "album", &png(&img)).unwrap();
        let blur = blur_of(&main);
        // Pin the old mtimes to a fixed historical value. No timing sleeps or
        // assumptions about the filesystem's timestamp resolution are needed.
        let old_time = UNIX_EPOCH + std::time::Duration::from_secs(1);
        File::options()
            .write(true)
            .open(&main)
            .unwrap()
            .set_modified(old_time)
            .unwrap();
        File::options()
            .write(true)
            .open(&blur)
            .unwrap()
            .set_modified(old_time)
            .unwrap();
        let old_main_version = file_version(&main).unwrap();
        let old_blur_version = file_version(&blur).unwrap();
        assert_eq!(
            old_main_version,
            format!("1000000000-{}", fs::metadata(&main).unwrap().len())
        );
        assert_eq!(save_cover(&dir.0, "album", &png(&frame(2))).unwrap(), main);
        assert_ne!(file_version(&main).unwrap(), old_main_version);
        assert_ne!(file_version(&blur).unwrap(), old_blur_version);
        assert!(file_version(&dir.0.join("missing.jpg")).is_none());
        assert_no_temps(&dir.0);
    }
}
