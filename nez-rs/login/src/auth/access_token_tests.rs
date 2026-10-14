use super::*;

#[test]
fn classifies_personal_access_tokens_by_prefix() {
    assert!(matches!(
        classify_nez_access_token("at-example"),
        NezAccessToken::PersonalAccessToken("at-example")
    ));
    assert!(matches!(
        classify_nez_access_token("header.payload.signature"),
        NezAccessToken::AgentIdentityJwt("header.payload.signature")
    ));
}
