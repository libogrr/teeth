use std::cell::RefCell;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use teeth::files::{
    ERROR_NOT_SAME_DEVICE, LocalTime, move_or_copy, move_or_copy_with, new_temp_path, now_local,
    orphans, suggested_name, temp_dir, temp_file_name,
};
use teeth::wav::part_path;

fn sample() -> LocalTime {
    LocalTime { year: 2026, month: 10, day: 25, hour: 14, minute: 32, second: 5 }
}

#[test]
fn names_follow_spec() {
    let t = sample();
    assert_eq!(temp_file_name(&t), "toma-20261025-143205.wav");
    assert_eq!(suggested_name(&t), "toma 2026-10-25 14-32-05.wav");
}

#[test]
fn temp_path_avoids_collisions() {
    let dir = tempfile::tempdir().unwrap();
    let t = sample();
    assert_eq!(new_temp_path(dir.path(), &t), dir.path().join("toma-20261025-143205.wav"));
    File::create(dir.path().join("toma-20261025-143205.wav")).unwrap();
    assert_eq!(new_temp_path(dir.path(), &t), dir.path().join("toma-20261025-143205-2.wav"));
    File::create(dir.path().join("toma-20261025-143205-2.wav")).unwrap();
    assert_eq!(new_temp_path(dir.path(), &t), dir.path().join("toma-20261025-143205-3.wav"));
}

#[test]
fn orphans_are_wav_files_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    assert!(orphans(dir.path()).is_empty());
    assert!(orphans(&dir.path().join("no_existe")).is_empty());

    let base = SystemTime::now() - Duration::from_secs(1000);
    let make = |name: &str, secs: u64| {
        let f = File::create(dir.path().join(name)).unwrap();
        f.set_modified(base + Duration::from_secs(secs)).unwrap();
    };
    make("a.wav", 0);
    make("c.WAV", 50);
    make("b.wav", 100);
    make("d.wav.part", 200);
    make("e.txt", 300);
    fs::create_dir(dir.path().join("sub.wav")).unwrap();
    let names: Vec<_> = orphans(dir.path()).iter().map(|p| p.file_name().unwrap().to_owned()).collect();
    assert_eq!(names, ["b.wav", "c.WAV", "a.wav"]);
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

/// A temp take and an existing destination with different contents.
fn src_and_dest(dir: &Path) -> (PathBuf, PathBuf) {
    let src = dir.join("toma-temp.wav");
    let dest = dir.join("dest.wav");
    fs::write(&src, "nuevo").unwrap();
    fs::write(&dest, "viejo").unwrap();
    (src, dest)
}

fn other_volume() -> io::Error {
    io::Error::from_raw_os_error(ERROR_NOT_SAME_DEVICE)
}

#[test]
fn move_or_copy_renames_on_the_same_volume() {
    let dir = tempfile::tempdir().unwrap();
    let (src, dest) = src_and_dest(dir.path());
    move_or_copy(&src, &dest).unwrap();
    assert_eq!(fs::read_to_string(&dest).unwrap(), "nuevo");
    assert_eq!(files_in(dir.path()), ["dest.wav"]);
}

#[test]
fn move_or_copy_across_volumes_copies_to_part_then_renames() {
    let dir = tempfile::tempdir().unwrap();
    let (src, dest) = src_and_dest(dir.path());
    let calls = RefCell::new(Vec::new());
    let rename = |a: &Path, b: &Path| {
        calls.borrow_mut().push(format!("rename {} -> {}", a.display(), b.display()));
        if a == src { Err(other_volume()) } else { fs::rename(a, b) }
    };
    let copy = |a: &Path, b: &Path| {
        calls.borrow_mut().push(format!("copy {} -> {}", a.display(), b.display()));
        // The temp is still there while it is copied, and the old destination too.
        assert_eq!(fs::read_to_string(&dest).unwrap(), "viejo");
        fs::copy(a, b).map(|_| ())
    };
    move_or_copy_with(&src, &dest, rename, copy).unwrap();
    let part = part_path(&dest);
    assert_eq!(
        calls.into_inner(),
        [
            format!("rename {} -> {}", src.display(), dest.display()),
            format!("copy {} -> {}", src.display(), part.display()),
            format!("rename {} -> {}", part.display(), dest.display()),
        ]
    );
    assert_eq!(fs::read_to_string(&dest).unwrap(), "nuevo");
    assert_eq!(files_in(dir.path()), ["dest.wav"], "temp deleted, no .part left");
}

#[test]
fn failed_cross_volume_copy_keeps_temp_and_destination() {
    let dir = tempfile::tempdir().unwrap();
    let (src, dest) = src_and_dest(dir.path());
    let rename = |a: &Path, b: &Path| if a == src { Err(other_volume()) } else { fs::rename(a, b) };
    let copy = |_: &Path, b: &Path| {
        fs::write(b, "nue").unwrap(); // half written, then the disk fills up
        Err(io::Error::other("disco lleno"))
    };
    let e = move_or_copy_with(&src, &dest, rename, copy).unwrap_err();
    assert_eq!(e.to_string(), "disco lleno");
    assert_eq!(fs::read_to_string(&src).unwrap(), "nuevo");
    assert_eq!(fs::read_to_string(&dest).unwrap(), "viejo");
    assert_eq!(files_in(dir.path()), ["dest.wav", "toma-temp.wav"]);
}

#[test]
fn failed_final_rename_keeps_temp_and_destination() {
    let dir = tempfile::tempdir().unwrap();
    let (src, dest) = src_and_dest(dir.path());
    let rename = |a: &Path, _: &Path| {
        Err(if a == src { other_volume() } else { io::Error::other("acceso denegado") })
    };
    let copy = |a: &Path, b: &Path| fs::copy(a, b).map(|_| ());
    let e = move_or_copy_with(&src, &dest, rename, copy).unwrap_err();
    assert_eq!(e.to_string(), "acceso denegado");
    assert_eq!(fs::read_to_string(&src).unwrap(), "nuevo");
    assert_eq!(fs::read_to_string(&dest).unwrap(), "viejo");
    assert_eq!(files_in(dir.path()), ["dest.wav", "toma-temp.wav"]);
}

#[test]
fn other_rename_errors_do_not_copy() {
    let dir = tempfile::tempdir().unwrap();
    let (src, dest) = src_and_dest(dir.path());
    let rename = |_: &Path, _: &Path| Err(io::Error::from_raw_os_error(5));
    let copy = |_: &Path, _: &Path| -> io::Result<()> { panic!("must not copy") };
    let e = move_or_copy_with(&src, &dest, rename, copy).unwrap_err();
    assert_eq!(e.raw_os_error(), Some(5));
    assert_eq!(files_in(dir.path()), ["dest.wav", "toma-temp.wav"]);
}

#[test]
fn now_local_and_temp_dir_are_sane() {
    let t = now_local();
    assert!(t.year >= 2024 && (1..=12).contains(&t.month) && (1..=31).contains(&t.day));
    assert!(t.hour < 24 && t.minute < 60 && t.second < 61);
    let d = temp_dir();
    assert!(d.is_dir());
    assert!(d.ends_with("Teeth"));
}

#[test]
fn looking_in_a_folder_that_does_not_exist_finds_nothing_and_does_not_create_it() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("Teeth");
    assert!(orphans(&missing).is_empty());
    assert!(!missing.exists());
}
