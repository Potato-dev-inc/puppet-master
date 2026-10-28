use super::*;

fn user(id: &str, text: &str) -> ChatItem {
    ChatItem::User { id: id.into(), at: None, text: text.into() }
}

fn ids(delta: &Delta) -> Vec<&str> {
    delta.items.iter().map(|item| item.id()).collect()
}

#[test]
fn revisions_only_go_up() {
    let a = next_rev();
    let b = next_rev();
    assert!(b > a);
}

#[test]
fn since_returns_only_what_is_newer() {
    let mut items = Items::new();
    items.push(user("1", "one"));
    items.push(user("2", "two"));
    let first = items.since(None, 300);
    assert_eq!(ids(&first), ["1", "2"]);
    assert!(!first.reset);
    let none = items.since(Some(first.rev), 300);
    assert!(none.items.is_empty() && !none.reset);
    assert_eq!(none.rev, first.rev);
    items.push(user("3", "three"));
    let next = items.since(Some(first.rev), 300);
    assert_eq!(ids(&next), ["3"]);
    assert!(next.rev > first.rev && !next.reset);
}

#[test]
fn a_changed_item_comes_again_under_its_id_and_an_equal_one_does_not() {
    let mut items = Items::new();
    items.push(user("1", "one"));
    items.push(user("2", "tw"));
    let seen = items.since(None, 300);
    items.upsert(&user("2", "tw"));
    assert!(items.since(Some(seen.rev), 300).items.is_empty(), "the same item is not a change");
    items.upsert(&user("2", "two"));
    let delta = items.since(Some(seen.rev), 300);
    assert_eq!(delta.items, vec![user("2", "two")]);
    assert_eq!(items.all().len(), 2, "replaced in place, not added");
}

#[test]
fn clearing_the_list_resets_a_viewer_that_saw_it() {
    let mut items = Items::new();
    items.push(user("1", "one"));
    let seen = items.since(None, 300);
    items.clear();
    items.push(user("9", "new file"));
    let delta = items.since(Some(seen.rev), 300);
    assert!(delta.reset);
    assert_eq!(ids(&delta), ["9"]);
    // A viewer that saw the new list is not reset.
    assert!(!items.since(Some(delta.rev), 300).reset);
}

#[test]
fn a_revision_from_another_list_or_the_future_is_a_reset() {
    let mut items = Items::new();
    items.push(user("1", "one"));
    assert!(items.since(Some(1), 300).reset, "older than the list");
    assert!(items.since(Some(u64::MAX / 2), 300).reset, "from the future");
}

#[test]
fn the_cap_keeps_the_newest_and_a_gap_is_a_reset() {
    let mut items = Items::new();
    for n in 0..4 {
        items.push(user(&n.to_string(), "x"));
    }
    let seen = items.since(None, 300);
    for n in 4..10 {
        items.push(user(&n.to_string(), "x"));
    }
    let first = items.since(None, 4);
    assert_eq!(ids(&first), ["6", "7", "8", "9"]);
    assert!(!first.reset);
    // Six changes behind with room for four: the viewer must start over.
    let behind = items.since(Some(seen.rev), 4);
    assert!(behind.reset);
    assert_eq!(ids(&behind), ["6", "7", "8", "9"]);
    // Four behind fits.
    let near = items.since(Some(items.revs[5]), 4);
    assert!(!near.reset);
    assert_eq!(ids(&near), ["6", "7", "8", "9"]);
}

#[test]
fn dropping_the_oldest_keeps_revisions_in_step() {
    let mut items = Items::new();
    for n in 0..5 {
        items.push(user(&n.to_string(), "x"));
    }
    let seen = items.since(None, 300);
    items.drop_oldest(2);
    items.push(user("5", "x"));
    assert_eq!(ids(&items.since(Some(seen.rev), 300)), ["5"]);
    assert_eq!(items.len(), 4);
}

#[test]
fn leases_run_out_and_plain_holders_do_not() {
    let now = Instant::now();
    let mut holders = Holders::default();
    holders.add(Hold::lease("phone", Duration::from_secs(60)), now);
    holders.add("window".into(), now);
    assert!(!holders.none_left(now + Duration::from_secs(61)), "the window still holds");
    holders.remove("window");
    assert!(holders.none_left(now + Duration::from_secs(61)), "the lease ran out");
    holders.add(Hold::lease("phone", Duration::from_secs(60)), now);
    holders.add(Hold::lease("phone", Duration::from_secs(60)), now + Duration::from_secs(50));
    assert!(!holders.none_left(now + Duration::from_secs(100)), "a renewed lease lasts from the renewal");
}

#[test]
fn a_delta_names_the_file_its_items_were_read_from() {
    let mut items = Items::new();
    assert_eq!(items.since(None, 10).file, None);
    items.set_file(Some(std::path::PathBuf::from("/sessions/a.jsonl")));
    items.push(user("1", "one"));
    assert_eq!(items.since(None, 10).file.as_deref(), Some(std::path::Path::new("/sessions/a.jsonl")));
    items.clear();
    items.set_file(Some(std::path::PathBuf::from("/sessions/b.jsonl")));
    assert_eq!(items.since(Some(1), 10).file.as_deref(), Some(std::path::Path::new("/sessions/b.jsonl")));
}
