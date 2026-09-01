use super::super::{ObservedRequest, Outcome, RequestStore, RuntimeMeasurements};
use super::{GROUP_SIZE, UNGROUPED_COMPACT_THRESHOLD};
use std::fs;
use std::time::Instant;
use uuid::Uuid;

fn seed_terminal(store: &RequestStore, index: usize) -> (String, std::path::PathBuf) {
    let (request, _) = store
        .begin(ObservedRequest {
            host_hint: Some("example.test"),
            ..ObservedRequest::test("GET", &format!("/r{index}"))
        })
        .unwrap();
    let id = request.id.clone();
    store
        .finish(
            &request,
            Instant::now(),
            &RuntimeMeasurements::default(),
            Outcome::Completed,
            None,
        )
        .unwrap();
    let old = request.locator.path();
    let timestamp = format!("20260831T000000.{index:03}Z");
    let renamed = store.root().join(format!("{timestamp}-example.test-{id}"));
    fs::rename(old, &renamed).unwrap();
    (id, renamed)
}

fn root_names(store: &RequestStore) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(store.root())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

fn group_name(timestamp: &str, count: usize) -> String {
    format!("{timestamp}-{count}")
}

#[test]
fn compact_moves_oldest_eligible_requests_into_a_named_group() {
    let temp = tempfile::tempdir().unwrap();
    let store = RequestStore::open(temp.path()).unwrap();
    let total = UNGROUPED_COMPACT_THRESHOLD + 1;
    let mut ids = Vec::with_capacity(total);
    for index in 0..total {
        ids.push(seed_terminal(&store, index).0);
    }

    store.compact_once().unwrap();

    let names = root_names(&store);
    let group = group_name("20260831T000000.000Z", GROUP_SIZE);
    assert!(names.contains(&group), "{names:?}");
    let hot = names.iter().filter(|name| *name != &group).count();
    assert_eq!(hot, total - GROUP_SIZE);

    let grouped = store.root().join(&group);
    assert_eq!(fs::read_dir(&grouped).unwrap().count(), GROUP_SIZE);
    store.find(&ids[0]).unwrap();
    store.find(ids.last().unwrap()).unwrap();

    let page = store.list_page(0, 50).unwrap();
    assert_eq!(page.total, total);
    assert_eq!(page.requests.len(), 50);
    assert_eq!(page.deletable_count, total);
}

#[test]
fn compact_skips_active_prefixed_and_in_process_requests() {
    let temp = tempfile::tempdir().unwrap();
    let store = RequestStore::open(temp.path()).unwrap();
    for index in 0..(UNGROUPED_COMPACT_THRESHOLD + 1) {
        store
            .begin(ObservedRequest {
                host_hint: Some("example.test"),
                ..ObservedRequest::test("GET", &format!("/live{index}"))
            })
            .unwrap();
    }

    store.compact_once().unwrap();

    assert!(
        root_names(&store)
            .iter()
            .all(|name| name.starts_with("active-"))
    );
    assert_eq!(
        store.list_page(0, 1).unwrap().total,
        UNGROUPED_COMPACT_THRESHOLD + 1
    );
}

#[test]
fn compact_does_not_create_a_partial_group() {
    let temp = tempfile::tempdir().unwrap();
    let store = RequestStore::open(temp.path()).unwrap();
    for index in 0..(GROUP_SIZE - 1) {
        seed_terminal(&store, index);
    }
    store.compact_once().unwrap();
    assert!(
        root_names(&store)
            .iter()
            .all(|name| parse_count_suffix(name).is_none())
    );
}

fn parse_count_suffix(name: &str) -> Option<usize> {
    let (timestamp, count) = name.split_once('-')?;
    if timestamp.len() != 20 || !count.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    count.parse().ok()
}

#[test]
fn repair_rolls_back_an_unfinished_grouping_directory() {
    let temp = tempfile::tempdir().unwrap();
    let store = RequestStore::open(temp.path()).unwrap();
    let (id, path) = seed_terminal(&store, 0);
    let tmp = store.root().join(format!(".grouping-{}", Uuid::new_v4()));
    fs::create_dir(&tmp).unwrap();
    fs::rename(&path, tmp.join(path.file_name().unwrap())).unwrap();
    assert!(store.find(&id).is_ok());

    store.compact_once().unwrap();

    assert!(!tmp.exists());
    assert_eq!(store.find(&id).unwrap().request.id, id);
}

#[test]
fn repair_renames_a_group_suffix_to_match_its_children() {
    let temp = tempfile::tempdir().unwrap();
    let store = RequestStore::open(temp.path()).unwrap();
    let total = UNGROUPED_COMPACT_THRESHOLD + 1;
    for index in 0..total {
        seed_terminal(&store, index);
    }
    store.compact_once().unwrap();
    let group = store
        .root()
        .join(group_name("20260831T000000.000Z", GROUP_SIZE));
    let stale = store
        .root()
        .join(group_name("20260831T000000.000Z", GROUP_SIZE + 5));
    fs::rename(&group, &stale).unwrap();

    store.compact_once().unwrap();

    assert!(!stale.exists());
    assert!(group.exists());
}

#[test]
fn list_page_skips_groups_by_count_and_delete_updates_the_suffix() {
    let temp = tempfile::tempdir().unwrap();
    let store = RequestStore::open(temp.path()).unwrap();
    let total = UNGROUPED_COMPACT_THRESHOLD + 1;
    let mut ids = Vec::with_capacity(total);
    for index in 0..total {
        ids.push(seed_terminal(&store, index).0);
    }
    store.compact_once().unwrap();

    let grouped_summary = store
        .root()
        .join(group_name("20260831T000000.000Z", GROUP_SIZE))
        .join(format!(
            "20260831T000000.{:03}Z-example.test-{}",
            GROUP_SIZE - 1,
            ids[GROUP_SIZE - 1]
        ))
        .join("summary.json");
    let original_summary = fs::read(&grouped_summary).unwrap();
    fs::write(&grouped_summary, b"not json").unwrap();

    let first = store.list_page(0, 50).unwrap();
    assert_eq!(first.total, total);
    assert_eq!(first.requests.len(), 50);

    let grouped_page = store.list_page(total - GROUP_SIZE, 50).unwrap();
    assert!(
        grouped_page.requests.len() < 50,
        "a corrupt grouped summary must not be required to count the collection"
    );

    fs::write(&grouped_summary, original_summary).unwrap();

    store.delete_ids(&[ids[2].clone()]).unwrap();
    assert!(
        store
            .root()
            .join(group_name("20260831T000000.000Z", GROUP_SIZE - 1))
            .is_dir()
    );
    assert_eq!(store.list_page(0, 1).unwrap().total, total - 1);
}

#[test]
fn deleting_the_last_grouped_request_removes_the_group() {
    let temp = tempfile::tempdir().unwrap();
    let store = RequestStore::open(temp.path()).unwrap();
    let total = UNGROUPED_COMPACT_THRESHOLD + 1;
    let mut ids = Vec::with_capacity(total);
    for index in 0..total {
        ids.push(seed_terminal(&store, index).0);
    }
    store.compact_once().unwrap();
    store.delete_ids(&ids[..GROUP_SIZE]).unwrap();
    assert!(
        !store
            .root()
            .join(group_name("20260831T000000.000Z", GROUP_SIZE))
            .exists()
    );
    assert_eq!(store.list_page(0, 1).unwrap().total, total - GROUP_SIZE);
}
