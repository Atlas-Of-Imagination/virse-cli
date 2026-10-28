use uuid::Uuid;
use virse_api::{ShareAddress, absolute_share_url};

#[test]
fn share_addresses_preserve_canvas_binding_and_reject_ambiguous_tokens() {
    let id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    assert_eq!(
        ShareAddress::parse("abc_DEF-123").unwrap(),
        ShareAddress::Token("abc_DEF-123".into())
    );
    assert_eq!(
        ShareAddress::parse("https://web.example/s/abc_DEF-123").unwrap(),
        ShareAddress::Token("abc_DEF-123".into())
    );
    assert_eq!(
        ShareAddress::parse(&format!(
            "https://web.example/canvas/{id}?share=abc_DEF-123"
        ))
        .unwrap(),
        ShareAddress::Canvas {
            canvas_id: id,
            token: Some("abc_DEF-123".into())
        }
    );
    assert!(
        ShareAddress::parse(&format!("/canvas/{id}"))
            .unwrap()
            .claim_target()
            .is_err()
    );
    for value in [
        format!("/canvas/{id}?share=one&share=two"),
        format!("/canvas/{id}?share="),
        "https://user:password@web.example/s/abc".into(),
        "file:///s/abc".into(),
        "//web.example/s/abc".into(),
        "/s/a%2Fb".into(),
    ] {
        assert!(ShareAddress::parse(&value).is_err(), "{value}");
    }
    assert_eq!(
        absolute_share_url(&format!("/canvas/{id}?share=abc"), "https://web.example").unwrap(),
        format!("https://web.example/canvas/{id}?share=abc")
    );
    assert!(absolute_share_url("//other.example/s/abc", "https://web.example").is_err());
    assert!(absolute_share_url("/s/abc", "https://web.example/subpath").is_err());
}
