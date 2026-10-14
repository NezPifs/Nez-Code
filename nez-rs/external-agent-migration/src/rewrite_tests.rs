use super::*;
use pretty_assertions::assert_eq;

const PROFILE: RewriteProfile = RewriteProfile::new("SOURCE.md", &["source agent"])
    .with_case_sensitive_term_variants(&["Source"]);

#[test]
fn rewrites_terms_only_at_word_boundaries() {
    assert_eq!(
        PROFILE.rewrite("SOURCE.md Source source agent source_agent Source: café ‘Source’"),
        "AGENTS.md Nez Nez source_agent Nez: café ‘Nez’"
    );
}

#[test]
fn preserves_urls_and_emails_while_rewriting_surrounding_text() {
    assert_eq!(
        PROFILE.rewrite(
            r#"Source: https://Source.example/SOURCE.md?product=Source#Source
[Source](HTTPS://Source.example/a_(Source)/source.md "Source")Source
<http://Source.example/SOURCE.md>，Source
“https://Source.example/é/SOURCE.md” Source
`file:///Source/SOURCE.md` Source://Source/SOURCE.md
[Source](mailto:Source@example.com) [SOURCE.md](./SOURCE.md)
WWW.Source.example/SOURCE.md Source
https://Source.example/it's/SOURCE.md Source
<Source@example.com> Source@example.com support@Source.example Source
[Source](/Source/reference) [SOURCE.md](./Source/SOURCE.md)
SOURCE.md?section=details
[Source](#Source)
<a id="Source"></a>
![Source](https://example.com/Source/SOURCE.md "Source")
[![logo][SOURCE.md logo]](https://Source.example/Source)

[SOURCE.md logo]: ./logo.svg

| URL | Agent | Instructions |
| --- | --- | --- |
|https://example.com|Source|[docs](SOURCE.md)|

[Source][Source] [Source][] [Source]

[Source]: https://example.com/Source/SOURCE.md "Source"
"#,
        ),
        r#"Nez: https://Source.example/SOURCE.md?product=Source#Source
[Nez](HTTPS://Source.example/a_(Source)/source.md "Nez")Nez
<http://Source.example/SOURCE.md>，Nez
“https://Source.example/é/SOURCE.md” Nez
`file:///Source/SOURCE.md` Source://Source/SOURCE.md
[Nez](mailto:Source@example.com) [AGENTS.md](./AGENTS.md)
WWW.Source.example/SOURCE.md Nez
https://Source.example/it's/SOURCE.md Nez
<Source@example.com> Source@example.com support@Source.example Nez
[Nez](/Nez/reference) [AGENTS.md](./Nez/AGENTS.md)
AGENTS.md?section=details
[Nez](#Nez)
<a id="Nez"></a>
![Nez](https://example.com/Source/SOURCE.md "Nez")
[![logo][AGENTS.md logo]](https://Source.example/Source)

[AGENTS.md logo]: ./logo.svg

| URL | Agent | Instructions |
| --- | --- | --- |
|https://example.com|Nez|[docs](AGENTS.md)|

[Nez][Nez] [Nez][] [Nez]

[Nez]: https://example.com/Source/SOURCE.md "Nez"
"#
    );
}
