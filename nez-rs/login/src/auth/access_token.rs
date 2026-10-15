const PERSONAL_ACCESS_TOKEN_PREFIX: &str = "at-";

pub(super) enum NezAccessToken<'a> {
    PersonalAccessToken(&'a str),
    AgentIdentityJwt(&'a str),
}

pub(super) fn classify_nez_access_token(access_token: &str) -> NezAccessToken<'_> {
    if access_token.starts_with(PERSONAL_ACCESS_TOKEN_PREFIX) {
        NezAccessToken::PersonalAccessToken(access_token)
    } else {
        NezAccessToken::AgentIdentityJwt(access_token)
    }
}

#[cfg(test)]
#[path = "access_token_tests.rs"]
mod tests;
