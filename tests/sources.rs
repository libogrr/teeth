use teeth::settings::Settings;
use teeth::sources::{ProcInfo, SessionInfo, group, pick_root, roots};

fn p(pid: u32, parent: u32, exe: &str, created: u64) -> ProcInfo {
    ProcInfo { pid, parent, exe: exe.into(), created }
}

#[test]
fn the_browser_root_is_chosen_over_its_audio_child() {
    let procs = [p(10, 1, "chrome.exe", 5), p(11, 10, "chrome.exe", 6), p(12, 10, "chrome.exe", 7)];
    assert_eq!(pick_root(&procs, "Chrome.EXE", &[12]), Some(10));
}

#[test]
fn among_several_roots_the_one_that_sounds_wins_then_the_oldest() {
    let procs = [p(20, 1, "app.exe", 9), p(21, 20, "app.exe", 9), p(30, 1, "app.exe", 3)];
    assert_eq!(pick_root(&procs, "app.exe", &[21]), Some(20));
    assert_eq!(pick_root(&procs, "app.exe", &[]), Some(30));
    assert_eq!(pick_root(&procs, "other.exe", &[]), None);
}

#[test]
fn an_orphan_whose_parent_is_gone_is_a_root() {
    let procs = [p(40, 999, "app.exe", 1)];
    assert_eq!(roots(&procs, "app.exe").len(), 1);
}

#[test]
fn sessions_group_by_app_sounding_first_without_teeth_or_system() {
    let procs = [
        p(1, 0, "teeth.exe", 1),
        p(2, 0, "tidal.exe", 1),
        p(3, 0, "chrome.exe", 1),
        p(4, 3, "chrome.exe", 2),
        p(5, 0, "discord.exe", 1),
    ];
    let s = |pid, active, system| SessionInfo { pid, active, audible: active, system };
    let sessions = [s(1, true, false), s(2, false, false), s(4, true, false), s(3, false, false), s(5, false, false), s(0, true, true)];
    let got = group(&sessions, &procs, 1);
    assert_eq!(got, vec![("chrome.exe".into(), true), ("discord.exe".into(), false), ("tidal.exe".into(), false)]);
}

#[test]
fn only_a_session_that_makes_sound_counts_as_sounding() {
    let procs = [p(2, 0, "tidal.exe", 1), p(3, 0, "chrome.exe", 1)];
    let s = |pid, audible| SessionInfo { pid, active: true, audible, system: false };
    let got = group(&[s(2, false), s(3, true)], &procs, 1);
    assert_eq!(got, vec![("chrome.exe".into(), true), ("tidal.exe".into(), false)]);
}

#[test]
fn a_session_without_a_process_is_left_out() {
    let procs = [p(0, 0, "[System Process]", 1), p(2, 0, "tidal.exe", 1)];
    let s = |pid| SessionInfo { pid, active: true, audible: true, system: false };
    assert_eq!(group(&[s(0), s(2)], &procs, 1), vec![("tidal.exe".into(), true)]);
}

#[test]
fn old_settings_without_a_source_still_load_and_a_source_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(&path, r#"{"last_folder":null,"window_size":[860.0,380.0]}"#).unwrap();
    assert_eq!(Settings::load_from(&path).source, None);
    let s = Settings { source: Some("chrome.exe".into()), ..Settings::default() };
    s.save_to(&path).unwrap();
    assert_eq!(Settings::load_from(&path).source.as_deref(), Some("chrome.exe"));
}
