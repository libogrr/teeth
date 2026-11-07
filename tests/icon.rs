#[test]
fn the_window_icon_decodes() {
    let icon = teeth::ui::theme::window_icon(include_bytes!("../assets/icon/teeth-window.png")).unwrap();
    assert_eq!((icon.width, icon.height), (256, 256));
}

#[test]
fn a_broken_icon_is_no_icon_and_no_crash() {
    assert!(teeth::ui::theme::window_icon(b"esto no es un png").is_none());
}

#[test]
fn the_splash_mark_decodes_and_is_taller_than_wide() {
    let mark = teeth::ui::splash::mark_image(include_bytes!("../assets/icon/teeth-mark.png")).unwrap();
    let [w, h] = mark.size;
    assert!(h > w, "{w} × {h}");
}

#[test]
fn a_broken_mark_is_no_splash_and_no_crash() {
    assert!(teeth::ui::splash::mark_image(b"esto no es un png").is_none());
}
