use glib::variant::ToVariant;

#[test]
fn string_iteration_preserves_values_in_optimized_builds() {
    let values = ["alpha", "", "服务器", "β"];
    let variant = values.to_variant();

    assert_eq!(
        variant.array_iter_str().unwrap().collect::<Vec<_>>(),
        values
    );
    assert_eq!(
        variant.array_iter_str().unwrap().rev().collect::<Vec<_>>(),
        values.into_iter().rev().collect::<Vec<_>>()
    );

    let mut iter = variant.array_iter_str().unwrap();
    assert_eq!(iter.next(), Some("alpha"));
    assert_eq!(iter.next_back(), Some("β"));
    assert_eq!(iter.nth(1), Some("服务器"));
    assert_eq!(iter.next_back(), None);
    assert_eq!(
        variant.array_iter_str().unwrap().nth_back(1),
        Some("服务器")
    );
    assert_eq!(variant.array_iter_str().unwrap().last(), Some("β"));

    let empty: [&str; 0] = [];
    let empty = empty.to_variant();
    assert_eq!(empty.array_iter_str().unwrap().next(), None);
}
