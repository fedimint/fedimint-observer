use super::{keyset_page, Page};

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

#[test]
fn keyset_pages_walk_the_whole_list_once_in_order() {
    // Sorted by score descending, then id: the key negates the score
    let rows: Vec<(i64, &str)> = vec![(5, "b"), (9, "a"), (5, "a"), (1, "c"), (9, "d")];
    let key = |row: &(i64, &str)| (-row.0, row.1.to_owned());
    let mut seen = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let page = keyset_page(rows.clone(), key, after.as_deref(), 2).unwrap();
        assert_eq!(page.total, 5);
        assert!(page.items.len() <= 2);
        seen.extend(page.items);
        match page.next_after {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    assert_eq!(seen, vec![(9, "a"), (9, "d"), (5, "a"), (5, "b"), (1, "c")]);
}

#[test]
fn a_row_removed_between_pages_does_not_skip_or_repeat_others() {
    let rows = vec![1, 2, 3, 4, 5, 6];
    let first = keyset_page(rows.clone(), |n| *n, None, 3).unwrap();
    assert_eq!(first.items, vec![1, 2, 3]);
    // Row 3 (the cursor's own row) and row 2 disappear before the next request
    let next = keyset_page(vec![1, 4, 5, 6], |n| *n, first.next_after.as_deref(), 3).unwrap();
    assert_eq!(next.items, vec![4, 5, 6]);
    assert_eq!(next.next_after, None);
}

#[test]
fn a_full_last_page_has_no_next_cursor_and_bad_cursors_are_rejected() {
    let page = keyset_page(vec![1, 2], |n| *n, None, 2).unwrap();
    assert_eq!(page.next_after, None);
    for cursor in ["not base64!", "bm90IGpzb24", &"A".repeat(2000)] {
        assert!(keyset_page(vec![1], |n| *n, Some(cursor), 2).is_err());
    }
}

#[test]
fn key_text_cuts_by_character_not_byte() {
    use super::{key_text, KEY_TEXT_CHARS};
    assert_eq!(key_text("short"), "short");
    let cut = key_text(&"𝔽".repeat(500));
    assert_eq!(cut.chars().count(), KEY_TEXT_CHARS);
    assert!(cut.chars().all(|c| c == '𝔽'));
}

/// The worst key a list builds: two cut texts that JSON escapes 6 bytes per
/// character, an id and the widest numbers
#[test]
fn the_largest_possible_cursor_is_accepted() {
    use super::{key_text, MAX_CURSOR_BYTES};
    let text = key_text(&"\u{1}".repeat(10_000));
    type Key = (bool, i128, Option<String>, String, String);
    let key = |n: i128| -> Key { (true, n, Some(text.clone()), text.clone(), "f".repeat(64)) };
    let rows = vec![i128::MIN, i128::MIN + 1];
    let page = keyset_page(rows, |n| key(*n), None, 1).unwrap();
    let cursor = page.next_after.unwrap();
    assert!(cursor.len() <= MAX_CURSOR_BYTES, "{} bytes", cursor.len());
    let next = keyset_page(
        vec![i128::MIN, i128::MIN + 1],
        |n| key(*n),
        Some(&cursor),
        1,
    )
    .unwrap();
    assert_eq!(next.items, vec![i128::MIN + 1]);
}
