use super::*;
use nez_keyring_store::tests::MockKeyringStore;
use pretty_assertions::assert_eq;

#[test]
fn keys_round_trip_through_the_credential_store() {
    let store = MockKeyringStore::default();
    assert_eq!(load_with(&store, "DEEPSEEK_API_KEY"), None);
    save_with(&store, "DEEPSEEK_API_KEY", "  sk-test \n").expect("save");
    assert_eq!(
        store
            .saved_value("provider-api-key:DEEPSEEK_API_KEY")
            .as_deref(),
        Some("sk-test")
    );
    assert_eq!(
        load_with(&store, "DEEPSEEK_API_KEY").as_deref(),
        Some("sk-test")
    );
    assert_eq!(delete_with(&store, "DEEPSEEK_API_KEY"), Ok(true));
    assert_eq!(load_with(&store, "DEEPSEEK_API_KEY"), None);
    assert_eq!(delete_with(&store, "DEEPSEEK_API_KEY"), Ok(false));
}

#[test]
fn empty_keys_are_rejected() {
    let store = MockKeyringStore::default();
    assert!(save_with(&store, "DEEPSEEK_API_KEY", "   ").is_err());
}

#[test]
fn credential_store_errors_read_as_no_key() {
    let store = MockKeyringStore::default();
    // The mock reports an injected error once, so set it before each call.
    let fail = || {
        store.set_error(
            "provider-api-key:DEEPSEEK_API_KEY",
            keyring::Error::PlatformFailure("no secret service".into()),
        );
    };
    fail();
    assert_eq!(load_with(&store, "DEEPSEEK_API_KEY"), None);
    fail();
    let err = save_with(&store, "DEEPSEEK_API_KEY", "sk").unwrap_err();
    assert!(err.contains("Set DEEPSEEK_API_KEY"), "{err}");
}
