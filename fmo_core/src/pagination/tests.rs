use super::Page;

#[test]
fn bounds_are_enforced_including_an_omitted_limit() {
    let mut page = Page::<i32> {
        after: None,
        limit: None,
    };
    assert_eq!(page.limit().unwrap(), 100);
    for limit in [0, 501, u16::MAX] {
        page.limit = Some(limit);
        assert!(page.limit().is_err());
    }
    page.limit = Some(500);
    assert_eq!(page.limit().unwrap(), 500);
}
