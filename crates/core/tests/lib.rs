use x3_core::*;
#[test]
fn settings_require_bounded_nonzero_values() {
    assert!(Settings::default().validate().is_ok());
    for settings in [
        Settings {
            token_limit: 0,
            ..Settings::default()
        },
        Settings {
            receive_seconds: 601,
            ..Settings::default()
        },
        Settings {
            full_refresh_pages: 0,
            ..Settings::default()
        },
    ] {
        assert_eq!(settings.validate(), Err(Error::Invalid));
    }
}
