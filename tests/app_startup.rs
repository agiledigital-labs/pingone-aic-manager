//! The real TUI startup constructor in an isolated project root.

#[test]
fn constructing_app_in_an_uninitialized_directory_leaves_no_project_marker() {
    let root = std::env::temp_dir().join(format!("aic-startup-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    pingone_aic_manager::config::set_project_root(root.clone()).unwrap();
    let app = pingone_aic_manager::app::App::new();
    let created_marker = root.join(".aic").exists();
    std::fs::remove_dir_all(root).unwrap();

    app.unwrap();
    assert!(!created_marker, "TUI startup created an empty .aic project");
}
