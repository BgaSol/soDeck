use super::*;
use std::sync::Barrier;
#[test]
fn cancellation_before_initial_subscribe_prevents_a_late_subscription() {
    let (hub, _, reader, now) = fixture();
    hub.unsubscribe(&reader, None, Some("cancelled"), now)
        .unwrap();
    assert_eq!(
        hub.subscribe(&reader, "cancelled", filter("t"), None, now)
            .unwrap_err()
            .code,
        "subscriptionExpired"
    );
}

fn context(workspace: &str, generation: u64) -> Context {
    Context {
        workspace_id: workspace.into(),
        server_id: "server".into(),
        connection_id: format!("connection-{generation}"),
        generation,
    }
}
fn producer(instance: &str, view: &str) -> Producer {
    Producer::Plugin {
        plugin_id: "test.plugin".into(),
        plugin_instance_id: instance.into(),
        view_id: view.into(),
    }
}
fn owner(context: &Context, instance: &str, view: &str) -> Owner {
    Owner {
        context: context.clone(),
        producer: producer(instance, view),
        target: DeliveryTarget::Process {
            instance_id: instance.into(),
            view_id: view.into(),
        },
    }
}
fn filter(topic: &str) -> Filter {
    Filter {
        topics: BTreeSet::from([topic.into()]),
        producer: None,
        key: None,
    }
}
fn fixture() -> (Arc<StateHub>, Owner, Owner, Instant) {
    let (hub, _) = StateHub::new();
    let hub = Arc::new(hub);
    let now = Instant::now();
    let context = context("w1", 1);
    hub.adopt_scope(context.clone(), now).unwrap();
    let writer = owner(&context, "producer", "p-view");
    let reader = owner(&context, "consumer", "c-view");
    for owner in [&writer, &reader] {
        hub.activate_instance(
            "test.plugin",
            match &owner.producer {
                Producer::Plugin {
                    plugin_instance_id, ..
                } => plugin_instance_id,
                _ => unreachable!(),
            },
        )
        .unwrap();
        hub.activate_view(owner.producer.clone(), &context).unwrap();
    }
    (hub, writer, reader, now)
}
fn put(hub: &StateHub, owner: &Owner, key: &str, expected: u64, value: Value, now: Instant) -> u64 {
    parse_revision(
        &hub.put(owner, "test.facts.v1", key, expected, value, 1000, now)
            .unwrap()["revision"],
    )
    .unwrap()
}
fn subscribe_live(hub: &StateHub, owner: &Owner, client: &str, now: Instant) -> String {
    let result = hub
        .subscribe(owner, client, filter("test.facts.v1"), None, now)
        .unwrap();
    let mut cursor = result["nextCursor"].as_str().map(String::from);
    while let Some(current) = cursor {
        let page = hub.page(owner, &current, false, now).unwrap();
        cursor = page["nextCursor"].as_str().map(String::from);
    }
    hub.subscribe(
        owner,
        client,
        filter("test.facts.v1"),
        Some(parse_revision(&result["revision"]).unwrap()),
        now,
    )
    .unwrap();
    result["subscriptionId"].as_str().unwrap().into()
}
fn ack(hub: &StateHub, owner: &Owner, frame: &Value, now: Instant) -> Value {
    hub.acknowledge(
        owner,
        frame["subscriptionId"].as_str().unwrap(),
        frame["batchId"].as_str().unwrap(),
        frame["part"].as_u64().unwrap() as usize,
        if frame["part"].as_u64().unwrap() + 1 == frame["parts"].as_u64().unwrap() {
            Some(parse_revision(&frame["toRevision"]).unwrap())
        } else {
            None
        },
        now,
    )
    .unwrap()
}

#[test]
fn null_absence_and_tombstones_are_distinct() {
    let (hub, writer, _, now) = fixture();
    assert_eq!(
        hub.get(&writer, "test.facts.v1", "key", None, now).unwrap()["record"],
        json!({"producer":writer.producer,"topic":"test.facts.v1","key":"key","revision":"0","present":false})
    );
    let revision = put(&hub, &writer, "key", 0, Value::Null, now);
    let current = hub.get(&writer, "test.facts.v1", "key", None, now).unwrap();
    assert_eq!(current["record"]["present"], true);
    assert!(current["record"].get("value").unwrap().is_null());
    let removed = hub
        .remove(&writer, "test.facts.v1", "key", revision, 1001, now)
        .unwrap();
    assert_eq!(removed, json!({"revision":"2","changed":true}));
    let tombstone = hub.get(&writer, "test.facts.v1", "key", None, now).unwrap();
    assert_eq!(tombstone["record"]["present"], false);
    assert_eq!(tombstone["record"]["revision"], "2");
    assert!(tombstone["record"].get("value").is_none());
    assert_eq!(
        hub.put(&writer, "test.facts.v1", "key", 0, json!("late"), 1002, now)
            .unwrap_err()
            .code,
        "revisionConflict"
    );
    assert_eq!(
        hub.remove(&writer, "test.facts.v1", "key", 2, 9999, now)
            .unwrap(),
        json!({"revision":"2","changed":false})
    );
    assert_eq!(put(&hub, &writer, "key", 2, json!("new"), now), 3);
}

#[test]
fn duplicate_value_does_not_refresh_time_or_emit() {
    let (hub, writer, reader, now) = fixture();
    put(&hub, &writer, "key", 0, json!({"observedAt":1000}), now);
    subscribe_live(&hub, &reader, "reader", now);
    let result = hub
        .put(
            &writer,
            "test.facts.v1",
            "key",
            1,
            json!({"observedAt":1000}),
            9999,
            now,
        )
        .unwrap();
    assert_eq!(result, json!({"revision":"1","changed":false}));
    assert_eq!(
        hub.get(&writer, "test.facts.v1", "key", None, now).unwrap()["record"]["receivedAt"],
        1000
    );
    assert!(hub.deliveries(now).is_empty());
    assert_eq!(
        hub.put(
            &writer,
            "test.facts.v1",
            "key",
            0,
            json!({"observedAt":1000}),
            1000,
            now
        )
        .unwrap_err()
        .code,
        "revisionConflict"
    );
}

#[test]
fn concurrent_cas_accepts_only_one_writer() {
    let (hub, writer, _, now) = fixture();
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = (0..2)
        .map(|value| {
            let (hub, writer, barrier) = (hub.clone(), writer.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                hub.put(&writer, "test.facts.v1", "key", 0, json!(value), 1000, now)
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = handles.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter_map(|r| r.as_ref().err())
            .next()
            .unwrap()
            .code,
        "revisionConflict"
    );
    assert_eq!(
        hub.get(&writer, "test.facts.v1", "key", None, now).unwrap()["record"]["revision"],
        "1"
    );
}

#[test]
fn unknown_commit_retry_cannot_overwrite_a_newer_intent() {
    let (hub, writer, _, now) = fixture();
    put(&hub, &writer, "key", 0, json!("first"), now);
    put(&hub, &writer, "key", 1, json!("latest"), now);
    let error = hub
        .put(
            &writer,
            "test.facts.v1",
            "key",
            0,
            json!("first"),
            1001,
            now,
        )
        .unwrap_err();
    assert_eq!(error.data.unwrap()["revision"], "2");
    assert_eq!(
        hub.get(&writer, "test.facts.v1", "key", None, now).unwrap()["record"]["value"],
        "latest"
    );
}

#[test]
fn revisions_remain_exact_above_javascript_precision() {
    for input in ["0", "9007199254740993", "18446744073709551615"] {
        assert_eq!(parse_revision(&json!(input)).unwrap().to_string(), input);
    }
    for input in [
        json!(1),
        json!(1.0),
        json!(null),
        json!("01"),
        json!("+1"),
        json!(" 1"),
        json!(""),
        json!("18446744073709551616"),
    ] {
        assert_eq!(parse_revision(&input).unwrap_err().code, "invalidParams");
    }
    let (hub, writer, reader, now) = fixture();
    hub.inner
        .lock()
        .unwrap()
        .scopes
        .get_mut("w1")
        .unwrap()
        .revision = 9_007_199_254_740_992;
    subscribe_live(&hub, &reader, "reader", now);
    assert_eq!(
        put(&hub, &writer, "key", 0, json!(1), now),
        9_007_199_254_740_993
    );
    let delivery = hub.deliveries(now).pop().unwrap();
    assert_eq!(delivery.frame["toRevision"], "9007199254740993");
    assert_eq!(
        ack(&hub, &reader, &delivery.frame, now)["confirmedRevision"],
        "9007199254740993"
    );
}

#[test]
fn subscribe_snapshot_and_later_changes_have_no_gap() {
    let (hub, writer, reader, now) = fixture();
    for i in 0..512 {
        put(
            &hub,
            &writer,
            &format!("key-{i:03}"),
            0,
            json!({"data":"x".repeat(500)}),
            now,
        );
    }
    let initial = hub
        .subscribe(&reader, "reader", filter("test.facts.v1"), None, now)
        .unwrap();
    assert!(initial["nextCursor"].is_string());
    let base = parse_revision(&initial["revision"]).unwrap();
    let key_revision = parse_revision(
        &hub.get(&writer, "test.facts.v1", "key-511", None, now)
            .unwrap()["record"]["revision"],
    )
    .unwrap();
    put(
        &hub,
        &writer,
        "key-511",
        key_revision,
        json!("updated during pagination"),
        now,
    );
    put(
        &hub,
        &writer,
        "late-key",
        0,
        json!("created during pagination"),
        now,
    );
    assert!(hub.deliveries(now).is_empty());
    assert_eq!(
        hub.subscribe(&reader, "reader", filter("test.facts.v1"), Some(base), now)
            .unwrap_err()
            .code,
        "invalidParams"
    );
    let mut records = initial["records"].as_array().unwrap().clone();
    let mut cursor = initial["nextCursor"].as_str().map(String::from);
    let mut last_cursor = String::new();
    while let Some(current) = cursor {
        last_cursor = current.clone();
        let page = hub.page(&reader, &current, false, now).unwrap();
        assert!(encoded_size(&page) + limits::ENVELOPE_BYTES <= limits::PROCESS_FRAME_BYTES);
        assert_eq!(page["revision"], initial["revision"]);
        records.extend(page["records"].as_array().unwrap().clone());
        cursor = page["nextCursor"].as_str().map(String::from);
        if cursor.is_none() {
            assert_eq!(hub.page(&reader, &current, false, now).unwrap(), page);
        }
    }
    assert_eq!(records.len(), 512);
    assert!(records.iter().all(|r| r["value"].is_object()));
    hub.subscribe(&reader, "reader", filter("test.facts.v1"), Some(base), now)
        .unwrap();
    assert_eq!(
        hub.page(&reader, &last_cursor, false, now)
            .unwrap_err()
            .code,
        "snapshotExpired"
    );
    let frame = hub.deliveries(now).pop().unwrap().frame;
    assert_eq!(frame["fromRevision"], "512");
    assert_eq!(frame["toRevision"], "514");
    let changes = frame["changes"].as_array().unwrap();
    assert_eq!(changes.len(), 2);
    assert!(changes.iter().any(|c| c["record"]["key"] == "late-key"));
    ack(&hub, &reader, &frame, now);
    assert!(hub.deliveries(now).is_empty());
}

#[test]
fn initialization_retries_preserve_snapshot_and_filter() {
    let (hub, writer, reader, now) = fixture();
    let initial = hub
        .subscribe(&reader, "reader", filter("test.facts.v1"), None, now)
        .unwrap();
    put(&hub, &writer, "key", 0, json!("late"), now);
    assert_eq!(
        hub.subscribe(&reader, "reader", filter("test.facts.v1"), None, now)
            .unwrap(),
        initial
    );
    assert_eq!(
        hub.subscribe(&reader, "reader", filter("other.v1"), None, now)
            .unwrap_err()
            .code,
        "invalidParams"
    );
    hub.subscribe(&reader, "reader", filter("test.facts.v1"), Some(0), now)
        .unwrap();
    assert_eq!(
        hub.subscribe(&reader, "reader", filter("test.facts.v1"), None, now)
            .unwrap_err()
            .code,
        "subscriptionInitialized"
    );
    assert_eq!(hub.usage().snapshots, 0);
}

#[test]
fn split_batch_only_advances_after_final_part_and_preserves_new_writes() {
    let (hub, writer, reader, now) = fixture();
    subscribe_live(&hub, &reader, "reader", now);
    for i in 0..512 {
        put(
            &hub,
            &writer,
            &format!("key-{i:03}"),
            0,
            json!({"data":"x".repeat(500)}),
            now,
        );
    }
    let first = hub.deliveries(now).pop().unwrap().frame;
    assert!(first["parts"].as_u64().unwrap() > 1);
    assert_eq!(first["part"], 0);
    assert_eq!(hub.deliveries(now)[0].frame, first);
    assert_eq!(
        hub.acknowledge(
            &reader,
            first["subscriptionId"].as_str().unwrap(),
            first["batchId"].as_str().unwrap(),
            1,
            None,
            now
        )
        .unwrap_err()
        .code,
        "invalidParams"
    );
    assert_eq!(
        hub.acknowledge(
            &reader,
            first["subscriptionId"].as_str().unwrap(),
            first["batchId"].as_str().unwrap(),
            0,
            Some(512),
            now
        )
        .unwrap_err()
        .code,
        "invalidParams"
    );
    assert_eq!(ack(&hub, &reader, &first, now)["confirmedRevision"], "0");
    assert_eq!(ack(&hub, &reader, &first, now)["confirmedRevision"], "0");
    put(&hub, &writer, "after-freeze", 0, json!(true), now);
    let mut seen = first["changes"].as_array().unwrap().len();
    let last;
    loop {
        let frame = hub.deliveries(now).pop().unwrap().frame;
        assert_eq!(frame["toRevision"], "512");
        assert!(encoded_size(&frame) + limits::ENVELOPE_BYTES <= limits::PROCESS_FRAME_BYTES);
        seen += frame["changes"].as_array().unwrap().len();
        let final_part = frame["part"].as_u64().unwrap() + 1 == frame["parts"].as_u64().unwrap();
        if final_part {
            // A lost final response can be fetched again, unchanged.
            assert_eq!(hub.deliveries(now)[0].frame, frame);
            assert_eq!(
                hub.subscribe(&reader, "reader", filter("test.facts.v1"), Some(0), now)
                    .unwrap()["confirmedRevision"],
                "0"
            );
        }
        let result = ack(&hub, &reader, &frame, now);
        if final_part {
            assert_eq!(result["confirmedRevision"], "512");
            last = frame;
            break;
        }
        assert_eq!(result["confirmedRevision"], "0");
    }
    assert_eq!(seen, 512);
    assert_eq!(ack(&hub, &reader, &last, now)["confirmedRevision"], "512");
    let next = hub.deliveries(now).pop().unwrap().frame;
    assert_eq!(next["fromRevision"], "512");
    assert_eq!(next["toRevision"], "513");
    assert_eq!(next["changes"].as_array().unwrap().len(), 1);
    assert_eq!(ack(&hub, &reader, &last, now)["confirmedRevision"], "512");
    ack(&hub, &reader, &next, now);
    assert!(hub.deliveries(now).is_empty());
}

#[test]
fn coalescing_keeps_final_record_and_removal() {
    let (hub, writer, reader, now) = fixture();
    subscribe_live(&hub, &reader, "reader", now);
    let revision = put(&hub, &writer, "key", 0, json!(1), now);
    let revision = put(&hub, &writer, "key", revision, json!(2), now);
    hub.remove(&writer, "test.facts.v1", "key", revision, 1002, now)
        .unwrap();
    let frame = hub.deliveries(now).pop().unwrap().frame;
    assert_eq!(frame["changes"].as_array().unwrap().len(), 1);
    assert_eq!(frame["changes"][0]["op"], "remove");
    assert_eq!(frame["changes"][0]["record"]["revision"], "3");
    ack(&hub, &reader, &frame, now);
    put(&hub, &writer, "key", 3, json!(3), now);
    let frame = hub.deliveries(now).pop().unwrap().frame;
    assert_eq!(frame["changes"][0]["op"], "put");
}

#[test]
fn filters_and_sources_isolate_facts_without_empty_batches() {
    let (hub, writer, reader, now) = fixture();
    let mut only_writer = filter("test.facts.v1");
    only_writer.producer = Some(writer.producer.clone());
    only_writer.key = Some("key".into());
    hub.subscribe(&reader, "reader", only_writer.clone(), None, now)
        .unwrap();
    hub.subscribe(&reader, "reader", only_writer, Some(0), now)
        .unwrap();
    put(&hub, &reader, "key", 0, json!("reader fact"), now);
    put(&hub, &writer, "other", 0, json!("other key"), now);
    hub.put(
        &writer,
        "other.topic.v1",
        "key",
        0,
        json!("other topic"),
        1000,
        now,
    )
    .unwrap();
    assert!(hub.deliveries(now).is_empty());
    assert_eq!(put(&hub, &writer, "key", 0, json!("writer fact"), now), 4);
    let frame = hub.deliveries(now).pop().unwrap().frame;
    assert_eq!(frame["fromRevision"], "0");
    assert_eq!(frame["toRevision"], "4");
    assert_eq!(frame["changes"].as_array().unwrap().len(), 1);
    assert_eq!(
        hub.get(&reader, "test.facts.v1", "key", Some(&writer.producer), now)
            .unwrap()["record"]["value"],
        "writer fact"
    );
    assert_eq!(
        hub.get(&reader, "test.facts.v1", "key", None, now).unwrap()["record"]["value"],
        "reader fact"
    );
    assert!(
        hub.list(&reader, filter("unknown.v1"), now).unwrap()["records"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn frozen_and_disconnected_are_independent_and_readable() {
    let (hub, writer, reader, now) = fixture();
    put(&hub, &writer, "key", 0, json!("old fact"), now);
    let id = subscribe_live(&hub, &reader, "reader", now);
    hub.set_scope_status(&writer.context, None, Some(true), now)
        .unwrap();
    hub.set_scope_status(&writer.context, Some(false), None, now)
        .unwrap();
    hub.set_scope_status(&writer.context, None, Some(false), now)
        .unwrap();
    let current = hub
        .get(&reader, "test.facts.v1", "key", Some(&writer.producer), now)
        .unwrap();
    assert_eq!(current["scope"]["state"], "disconnected");
    assert_eq!(current["scope"]["frozen"], false);
    assert_eq!(current["record"]["value"], "old fact");
    assert_eq!(
        hub.put(&writer, "test.facts.v1", "key", 1, json!("bad"), 1001, now)
            .unwrap_err()
            .code,
        "staleContext"
    );
    let frame = hub.deliveries(now).pop().unwrap().frame;
    assert_eq!(frame["changes"][0]["scope"]["connected"], false);
    assert_eq!(frame["changes"][0]["scope"]["frozen"], false);
    ack(&hub, &reader, &frame, now);
    hub.set_scope_status(&writer.context, None, Some(true), now)
        .unwrap();
    let revision = hub.inner.lock().unwrap().subscriptions[&id].confirmed_revision;
    assert!(hub
        .subscribe(
            &reader,
            "reader",
            filter("test.facts.v1"),
            Some(revision),
            now
        )
        .is_ok());
    assert_eq!(
        hub.subscribe(&reader, "new", filter("test.facts.v1"), None, now)
            .unwrap_err()
            .code,
        "workspaceClosing"
    );
    assert_eq!(
        hub.put(&writer, "test.facts.v1", "key", 1, json!("bad"), 1001, now)
            .unwrap_err()
            .code,
        "workspaceClosing"
    );
    hub.unsubscribe(&reader, Some(&id), None, now).unwrap();
    hub.unsubscribe(&reader, Some(&id), None, now).unwrap();
}

#[test]
fn reconnect_removes_old_versions_snapshots_and_subscriptions() {
    let (hub, writer, reader, now) = fixture();
    put(&hub, &writer, "key", 0, json!("old"), now);
    let initial = hub
        .subscribe(&reader, "reader", filter("test.facts.v1"), None, now)
        .unwrap();
    let next = context("w1", 2);
    hub.adopt_scope(next.clone(), now).unwrap();
    assert_eq!(hub.usage().records, 0);
    assert_eq!(hub.usage().snapshots, 0);
    assert_eq!(hub.usage().subscriptions, 0);
    assert_eq!(
        hub.put(&writer, "test.facts.v1", "key", 1, json!("late"), 1002, now)
            .unwrap_err()
            .code,
        "staleContext"
    );
    assert_eq!(
        hub.page(&reader, initial["snapshotId"].as_str().unwrap(), false, now)
            .unwrap_err()
            .code,
        "staleContext"
    );
    let new_reader = Owner {
        context: next.clone(),
        ..reader
    };
    hub.activate_view(new_reader.producer.clone(), &next)
        .unwrap();
    assert_eq!(
        hub.get(
            &new_reader,
            "test.facts.v1",
            "key",
            Some(&writer.producer),
            now
        )
        .unwrap()["record"]["revision"],
        "0"
    );
}

#[test]
fn same_server_workspaces_are_separate() {
    let (hub, writer, reader, now) = fixture();
    let second = context("w2", 1);
    hub.adopt_scope(second.clone(), now).unwrap();
    let other = owner(&second, "consumer", "second-view");
    hub.activate_view(other.producer.clone(), &second).unwrap();
    subscribe_live(&hub, &reader, "reader", now);
    subscribe_live(&hub, &other, "reader", now);
    put(&hub, &writer, "key", 0, json!(true), now);
    assert_eq!(hub.deliveries(now).len(), 1);
    assert_eq!(
        hub.get(&other, "test.facts.v1", "key", Some(&writer.producer), now)
            .unwrap()["record"]["present"],
        false
    );
}

#[test]
fn stopped_view_or_instance_cannot_recreate_its_contributions() {
    for stop_instance in [false, true] {
        let (hub, writer, reader, now) = fixture();
        put(&hub, &writer, "key", 0, json!(true), now);
        subscribe_live(&hub, &reader, "reader", now);
        subscribe_live(&hub, &writer, "writer-sub", now);
        let reader_retained = hub.usage().retained_bytes;
        if stop_instance {
            hub.stop_instance("test.plugin", "producer", 1001, now);
        } else {
            hub.remove_view(&writer.producer, 1001, now);
        }
        assert_eq!(hub.usage().records, 0);
        assert_eq!(hub.usage().subscriptions, 1);
        assert!(hub.usage().retained_bytes < reader_retained);
        let frame = hub.deliveries(now).pop().unwrap().frame;
        assert_eq!(frame["changes"][0]["op"], "remove");
        assert_eq!(frame["changes"][0]["reason"], "sourceStopped");
        assert_eq!(
            hub.put(&writer, "test.facts.v1", "key", 0, json!("late"), 1002, now)
                .unwrap_err()
                .code,
            "sourceStopped"
        );
        ack(&hub, &reader, &frame, now);
        hub.stop_instance("test.plugin", "producer", 1002, now);
        assert!(hub.deliveries(now).is_empty());
    }
}

#[test]
fn concurrent_stop_and_write_cannot_leave_residual_state() {
    let (hub, writer, _, now) = fixture();
    let barrier = Arc::new(Barrier::new(3));
    let writing = {
        let (hub, writer, barrier) = (hub.clone(), writer.clone(), barrier.clone());
        std::thread::spawn(move || {
            barrier.wait();
            hub.put(&writer, "test.facts.v1", "key", 0, json!(true), 1000, now)
        })
    };
    let stopping = {
        let (hub, barrier) = (hub.clone(), barrier.clone());
        std::thread::spawn(move || {
            barrier.wait();
            hub.stop_instance("test.plugin", "producer", 1001, now)
        })
    };
    barrier.wait();
    let _ = writing.join().unwrap();
    stopping.join().unwrap();
    assert_eq!(hub.usage().records, 0);
    assert_eq!(
        hub.put(&writer, "test.facts.v1", "key", 0, json!("late"), 1002, now)
            .unwrap_err()
            .code,
        "sourceStopped"
    );
}

#[test]
fn surface_teardown_preserves_logical_facts_but_revokes_its_subscriptions() {
    let (hub, writer, reader, now) = fixture();
    let source_surface = Owner {
        target: DeliveryTarget::Surface {
            window_id: "main".into(),
            surface_id: "surface-1".into(),
        },
        ..reader.clone()
    };
    let target_surface = Owner {
        target: DeliveryTarget::Surface {
            window_id: "secondary".into(),
            surface_id: "surface-2".into(),
        },
        ..reader.clone()
    };
    put(&hub, &writer, "key", 0, json!(true), now);
    let id = subscribe_live(&hub, &source_surface, "surface", now);
    let snapshot = hub
        .list(&source_surface, filter("test.facts.v1"), now)
        .unwrap();
    assert_eq!(
        hub.page(
            &target_surface,
            snapshot["snapshotId"].as_str().unwrap(),
            false,
            now
        )
        .unwrap_err()
        .code,
        "invalidParams"
    );
    hub.remove_surface("main", "surface-1", now);
    assert_eq!(hub.usage().records, 1);
    assert_eq!(hub.usage().snapshots, 0);
    assert_eq!(hub.usage().subscriptions, 0);
    assert_eq!(
        hub.acknowledge(&source_surface, &id, "old-batch", 0, Some(1), now)
            .unwrap_err()
            .code,
        "subscriptionExpired"
    );
    let id = subscribe_live(&hub, &target_surface, "surface", now);
    put(&hub, &writer, "next", 0, json!(true), now);
    let delivery = hub.deliveries(now).pop().unwrap();
    assert_eq!(delivery.subscription_id, id);
    assert_eq!(delivery.target, target_surface.target);
    assert_eq!(
        hub.acknowledge(
            &source_surface,
            &id,
            delivery.frame["batchId"].as_str().unwrap(),
            0,
            Some(2),
            now
        )
        .unwrap_err()
        .code,
        "invalidParams"
    );
    ack(&hub, &target_surface, &delivery.frame, now);
}

#[test]
fn leases_and_snapshot_idle_expire_without_business_writes() {
    let (hub, writer, reader, now) = fixture();
    let list = hub.list(&reader, filter("test.facts.v1"), now).unwrap();
    assert_eq!(
        hub.page(
            &reader,
            list["snapshotId"].as_str().unwrap(),
            false,
            now + limits::SNAPSHOT_IDLE
        )
        .unwrap_err()
        .code,
        "snapshotExpired"
    );
    let initial = hub
        .subscribe(&reader, "initializing", filter("test.facts.v1"), None, now)
        .unwrap();
    hub.expire(now + limits::SNAPSHOT_IDLE);
    assert_eq!(
        hub.subscribe(
            &reader,
            "initializing",
            filter("test.facts.v1"),
            None,
            now + limits::SNAPSHOT_IDLE
        )
        .unwrap_err()
        .code,
        "resetRequired"
    );
    assert_eq!(hub.usage().snapshots, 0);
    assert!(
        hub.deliveries(now + limits::SNAPSHOT_IDLE)[0].frame["resetRequired"]
            .as_bool()
            .unwrap()
    );
    hub.unsubscribe(
        &reader,
        Some(initial["subscriptionId"].as_str().unwrap()),
        None,
        now + limits::SNAPSHOT_IDLE,
    )
    .unwrap();
    assert_eq!(
        hub.subscribe(
            &reader,
            "initializing",
            filter("test.facts.v1"),
            None,
            now + limits::SNAPSHOT_IDLE
        )
        .unwrap_err()
        .code,
        "subscriptionExpired"
    );
    let id = subscribe_live(&hub, &reader, "live", now);
    hub.subscribe(
        &reader,
        "live",
        filter("test.facts.v1"),
        Some(0),
        now + std::time::Duration::from_secs(30),
    )
    .unwrap();
    assert!(hub
        .subscribe(
            &reader,
            "live",
            filter("test.facts.v1"),
            Some(0),
            now + std::time::Duration::from_secs(61)
        )
        .is_ok());
    let expiry = now + std::time::Duration::from_secs(121);
    hub.expire(expiry);
    assert_eq!(hub.usage().subscriptions, 0);
    assert_eq!(
        hub.subscribe(&reader, "live", filter("test.facts.v1"), None, expiry)
            .unwrap_err()
            .code,
        "subscriptionExpired"
    );
    assert_eq!(
        hub.acknowledge(&reader, &id, "old-batch", 0, Some(0), expiry)
            .unwrap_err()
            .code,
        "subscriptionExpired"
    );
    assert!(hub
        .get(&writer, "test.facts.v1", "key", None, expiry)
        .is_ok());
}

#[test]
fn snapshot_last_page_can_be_retried_and_released_idempotently() {
    let (hub, writer, reader, now) = fixture();
    put(&hub, &writer, "key", 0, json!("snapshot"), now);
    let list = hub.list(&reader, filter("test.facts.v1"), now).unwrap();
    let cursor = list["snapshotId"].as_str().unwrap();
    assert!(list["nextCursor"].is_null());
    put(&hub, &writer, "key", 1, json!("new"), now);
    assert_eq!(hub.page(&reader, cursor, false, now).unwrap(), list);
    hub.page(&reader, cursor, true, now).unwrap();
    hub.page(&reader, cursor, true, now).unwrap();
    assert_eq!(hub.usage().snapshots, 0);
    assert_eq!(
        hub.page(&reader, cursor, false, now).unwrap_err().code,
        "snapshotExpired"
    );
}

#[test]
fn invalid_metadata_depth_and_oversized_values_do_not_modify_current_fact() {
    let (hub, writer, _, now) = fixture();
    put(&hub, &writer, "key", 0, json!("original"), now);
    for topic in ["", "bad topic", "测试", &"x".repeat(129)] {
        assert_eq!(
            hub.put(&writer, topic, "key", 1, json!(1), 1001, now)
                .unwrap_err()
                .code,
            "invalidParams"
        );
    }
    for key in ["", "bad\0key", &"键".repeat(171)] {
        assert_eq!(
            hub.put(&writer, "test.facts.v1", key, 0, json!(1), 1001, now)
                .unwrap_err()
                .code,
            "invalidParams"
        );
    }
    let mut nested = json!(1);
    for _ in 0..33 {
        nested = json!([nested]);
    }
    assert_eq!(
        hub.put(&writer, "test.facts.v1", "key", 1, nested, 1001, now)
            .unwrap_err()
            .code,
        "invalidParams"
    );
    assert_eq!(
        hub.put(
            &writer,
            "test.facts.v1",
            "key",
            1,
            json!("x".repeat(limits::RECORD_BYTES)),
            1001,
            now
        )
        .unwrap_err()
        .code,
        "resourceLimit"
    );
    assert_eq!(
        hub.get(&writer, "test.facts.v1", "key", None, now).unwrap()["record"]["value"],
        "original"
    );
    assert_eq!(hub.usage().records, 1);
    let bad_target = Owner {
        target: DeliveryTarget::Process {
            instance_id: "other".into(),
            view_id: "p-view".into(),
        },
        ..writer.clone()
    };
    assert_eq!(
        hub.list(&bad_target, filter("test.facts.v1"), now)
            .unwrap_err()
            .code,
        "invalidParams"
    );
    let absent = owner(&writer.context, "producer", "unregistered-view");
    assert_eq!(
        hub.put(&absent, "test.facts.v1", "key", 0, json!(1), 1001, now)
            .unwrap_err()
            .code,
        "sourceStopped"
    );
    let mut invalid_filter = filter("test.facts.v1");
    invalid_filter.topics = (0..9).map(|i| format!("topic-{i}")).collect();
    assert_eq!(
        hub.list(&writer, invalid_filter, now).unwrap_err().code,
        "invalidParams"
    );
}

#[test]
fn full_record_encoding_fits_the_smallest_surface_frame() {
    let (hub, writer, reader, now) = fixture();
    let reader = Owner {
        target: DeliveryTarget::Surface {
            window_id: "w".repeat(128),
            surface_id: "s".repeat(128),
        },
        ..reader
    };
    subscribe_live(&hub, &reader, "c".repeat(128).as_str(), now);
    let key = "k".repeat(512);
    put(&hub, &writer, &key, 0, json!("x".repeat(15_000)), now);
    let list = hub.list(&reader, filter("test.facts.v1"), now).unwrap();
    assert!(encoded_size(&list) + limits::ENVELOPE_BYTES <= limits::SURFACE_FRAME_BYTES);
    let delivery = hub.deliveries(now).pop().unwrap();
    assert!(encoded_size(&delivery.frame) + limits::ENVELOPE_BYTES <= limits::SURFACE_FRAME_BYTES);
    assert!(
        encoded_size(
            &json!({"windowId":"w".repeat(128),"surfaceId":"s".repeat(128),"data":delivery.frame})
        ) + 1
            <= limits::SURFACE_FRAME_BYTES
    );
    assert!(
        encoded_size(
            &json!({"jsonrpc":"2.0","id":"ui-18446744073709551615","method":"state.changed","params":delivery.frame})
        ) + 1
            <= limits::PROCESS_FRAME_BYTES
    );
    ack(&hub, &reader, &delivery.frame, now);
}

#[test]
fn scope_budget_preserves_existing_value_and_tombstone_count_is_bounded() {
    let (hub, writer, _, now) = fixture();
    let value = json!("x".repeat(15_000));
    let mut accepted = 0;
    loop {
        match hub.put(
            &writer,
            "test.facts.v1",
            &format!("key-{accepted}"),
            0,
            value.clone(),
            1000,
            now,
        ) {
            Ok(_) => accepted += 1,
            Err(error) => {
                assert_eq!(error.code, "resourceLimit");
                break;
            }
        }
    }
    assert!(accepted > 250 && accepted < 300);
    assert!(hub.inner.lock().unwrap().scopes["w1"].bytes <= limits::SCOPE_BYTES);
    assert_eq!(hub.usage().records, accepted);
    assert_eq!(
        hub.get(&writer, "test.facts.v1", "key-0", None, now)
            .unwrap()["record"]["value"],
        value
    );
    hub.clear();
    hub.adopt_scope(writer.context.clone(), now).unwrap();
    hub.activate_instance("test.plugin", "producer").unwrap();
    hub.activate_view(writer.producer.clone(), &writer.context)
        .unwrap();
    for i in 0..limits::RECORDS {
        let key = i.to_string();
        let revision = put(&hub, &writer, &key, 0, json!(true), now);
        hub.remove(&writer, "test.facts.v1", &key, revision, 1001, now)
            .unwrap();
    }
    assert_eq!(hub.usage().records, limits::RECORDS);
    assert_eq!(
        hub.put(
            &writer,
            "test.facts.v1",
            "one-too-many",
            0,
            json!(true),
            1000,
            now
        )
        .unwrap_err()
        .code,
        "resourceLimit"
    );
    assert!(
        hub.list(&writer, filter("test.facts.v1"), now).unwrap()["records"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn old_snapshot_values_stay_charged_until_release() {
    let (hub, writer, reader, now) = fixture();
    put(&hub, &writer, "key", 0, json!("x".repeat(15_000)), now);
    let list = hub.list(&reader, filter("test.facts.v1"), now).unwrap();
    let transient = hub.usage().transient_bytes;
    assert!(transient > 15_000);
    put(&hub, &writer, "key", 1, json!(false), now);
    assert_eq!(hub.usage().transient_bytes, transient);
    assert_eq!(
        hub.page(&reader, list["snapshotId"].as_str().unwrap(), false, now)
            .unwrap()["records"][0]["value"],
        "x".repeat(15_000)
    );
    hub.page(&reader, list["snapshotId"].as_str().unwrap(), true, now)
        .unwrap();
    assert_eq!(hub.usage().transient_bytes, 0);
}

#[test]
fn snapshot_budget_and_global_current_budget_are_bounded() {
    let (hub, writer, reader, now) = fixture();
    for i in 0..128 {
        put(
            &hub,
            &writer,
            &i.to_string(),
            0,
            json!("x".repeat(15_000)),
            now,
        );
    }
    let mut snapshots = Vec::new();
    loop {
        match hub.list(&reader, filter("test.facts.v1"), now) {
            Ok(result) => snapshots.push(result["snapshotId"].as_str().unwrap().to_owned()),
            Err(error) => {
                assert_eq!(error.code, "resourceLimit");
                break;
            }
        }
    }
    assert!(snapshots.len() > 10 && snapshots.len() < 20);
    assert!(hub.usage().transient_bytes <= limits::TRANSIENT_BYTES);
    for id in snapshots {
        hub.page(&reader, &id, true, now).unwrap();
    }
    assert_eq!(hub.usage().transient_bytes, 0);
    let mut hit_global_limit = false;
    'scopes: for i in 2..=12 {
        let context = context(&format!("w{i}"), 1);
        hub.adopt_scope(context.clone(), now).unwrap();
        let other = owner(&context, "producer", &format!("view-{i}"));
        hub.activate_view(other.producer.clone(), &context).unwrap();
        for key in 0..250 {
            if let Err(error) = hub.put(
                &other,
                "test.facts.v1",
                &key.to_string(),
                0,
                json!("x".repeat(15_000)),
                1000,
                now,
            ) {
                assert_eq!(error.code, "resourceLimit");
                assert!(
                    hub.inner.lock().unwrap().scopes[&context.workspace_id].bytes
                        < limits::SCOPE_BYTES
                );
                hit_global_limit = true;
                break 'scopes;
            }
        }
    }
    assert!(hit_global_limit);
    assert!(hub.usage().retained_bytes <= limits::RETAINED_BYTES);
    hub.clear();
    assert_eq!(hub.usage(), Usage::default());
}

#[test]
fn slow_consumer_resets_and_new_subscription_resynchronizes() {
    let (hub, writer, reader, now) = fixture();
    let id = subscribe_live(&hub, &reader, "slow", now);
    for i in 0..128 {
        put(
            &hub,
            &writer,
            &i.to_string(),
            0,
            json!("x".repeat(9000)),
            now,
        );
    }
    let delivery = hub.deliveries(now).pop().unwrap();
    assert_eq!(delivery.subscription_id, id);
    assert_eq!(delivery.frame["resetRequired"], true);
    assert!(hub.usage().transient_bytes < 4096);
    assert_eq!(
        hub.subscribe(&reader, "slow", filter("test.facts.v1"), Some(0), now)
            .unwrap_err()
            .code,
        "resetRequired"
    );
    assert_eq!(
        hub.subscribe(&reader, "slow", filter("test.facts.v1"), None, now)
            .unwrap_err()
            .code,
        "resetRequired"
    );
    hub.unsubscribe(&reader, None, Some("slow"), now).unwrap();
    let next = subscribe_live(&hub, &reader, "recovered", now);
    assert_ne!(id, next);
    assert!(hub.deliveries(now).is_empty());
}

#[test]
fn global_pending_budget_resets_without_rolling_back_writes() {
    let (hub, writer, reader, now) = fixture();
    for i in 0..limits::INSTANCE_SUBSCRIPTIONS {
        subscribe_live(&hub, &reader, &format!("sub-{i}"), now);
    }
    assert_eq!(
        hub.subscribe(&reader, "too-many", filter("test.facts.v1"), None, now)
            .unwrap_err()
            .code,
        "resourceLimit"
    );
    for i in 0..80 {
        put(
            &hub,
            &writer,
            &i.to_string(),
            0,
            json!("x".repeat(15_000)),
            now,
        );
    }
    assert_eq!(hub.usage().records, 80);
    assert!(hub.usage().transient_bytes <= limits::TRANSIENT_BYTES);
    assert!(hub
        .deliveries(now)
        .iter()
        .any(|d| d.frame["resetRequired"] == true));
    hub.remove_view(&reader.producer, 1001, now);
    assert_eq!(hub.usage().transient_bytes, 0);
}

#[test]
fn exhausted_revision_rejects_writes_but_can_release_sources() {
    let (hub, writer, reader, now) = fixture();
    put(&hub, &writer, "key", 0, json!(true), now);
    hub.inner
        .lock()
        .unwrap()
        .scopes
        .get_mut("w1")
        .unwrap()
        .revision = u64::MAX;
    subscribe_live(&hub, &reader, "reader", now);
    assert_eq!(
        hub.put(&writer, "test.facts.v1", "key", 1, json!(false), 1001, now)
            .unwrap_err()
            .code,
        "resourceLimit"
    );
    hub.stop_instance("test.plugin", "producer", 1001, now);
    assert_eq!(hub.usage().records, 0);
    assert_eq!(hub.deliveries(now)[0].frame["resetRequired"], true);
}

#[test]
fn scope_close_and_clear_release_all_retained_resources() {
    let (hub, writer, reader, now) = fixture();
    let revision = put(&hub, &writer, "key", 0, json!(true), now);
    hub.remove(&writer, "test.facts.v1", "key", revision, 1001, now)
        .unwrap();
    subscribe_live(&hub, &reader, "reader", now);
    hub.list(&reader, filter("test.facts.v1"), now).unwrap();
    hub.unsubscribe(&reader, None, Some("reader"), now).unwrap();
    assert_eq!(hub.usage().retired_subscription_ids, 1);
    hub.remove_scope("w1", now);
    assert_eq!(hub.usage().records, 0);
    assert_eq!(hub.usage().snapshots, 0);
    assert_eq!(hub.usage().subscriptions, 0);
    assert_eq!(hub.usage().transient_bytes, 0);
    assert_eq!(
        hub.get(&reader, "test.facts.v1", "key", None, now)
            .unwrap_err()
            .code,
        "scopeClosed"
    );
    hub.stop_instance("test.plugin", "producer", 1002, now);
    hub.stop_instance("test.plugin", "consumer", 1002, now);
    assert_eq!(hub.usage(), Usage::default());
}

#[test]
fn wake_signal_is_coalesced_and_never_blocks_a_commit() {
    let (hub, receiver) = StateHub::new();
    let now = Instant::now();
    let context = context("w1", 1);
    hub.adopt_scope(context.clone(), now).unwrap();
    let owner = Owner {
        context,
        producer: Producer::Core,
        target: DeliveryTarget::Process {
            instance_id: "core".into(),
            view_id: "host".into(),
        },
    };
    for i in 0..10 {
        put(&hub, &owner, &i.to_string(), 0, json!(true), now);
    }
    receiver.try_recv().unwrap();
    assert_eq!(receiver.try_recv().unwrap_err(), mpsc::TryRecvError::Empty);
    assert_eq!(hub.usage().records, 10);
}

#[test]
fn active_batch_and_pending_values_share_one_backlog_budget() {
    let (hub, writer, reader, now) = fixture();
    subscribe_live(&hub, &reader, "reader", now);
    for i in 0..40 {
        put(
            &hub,
            &writer,
            &i.to_string(),
            0,
            json!("x".repeat(9000)),
            now,
        );
    }
    let frame = hub.deliveries(now).pop().unwrap().frame;
    assert!(frame.get("batchId").is_some());
    for i in 40..80 {
        put(
            &hub,
            &writer,
            &i.to_string(),
            0,
            json!("x".repeat(9000)),
            now,
        );
    }
    assert_eq!(hub.deliveries(now)[0].frame["resetRequired"], true);
    assert!(hub.usage().transient_bytes < 4096);
    assert_eq!(
        hub.acknowledge(
            &reader,
            frame["subscriptionId"].as_str().unwrap(),
            frame["batchId"].as_str().unwrap(),
            0,
            None,
            now
        )
        .unwrap_err()
        .code,
        "resetRequired"
    );
    assert_eq!(hub.usage().records, 80);
}

#[test]
fn source_stop_during_initialization_is_delivered_after_snapshot_confirmation() {
    let (hub, writer, reader, now) = fixture();
    put(&hub, &writer, "key", 0, json!(true), now);
    let initial = hub
        .subscribe(&reader, "reader", filter("test.facts.v1"), None, now)
        .unwrap();
    hub.stop_instance("test.plugin", "producer", 1001, now);
    assert_eq!(hub.usage().records, 0);
    assert!(hub.usage().transient_bytes > 0);
    assert!(hub.deliveries(now).is_empty());
    assert_eq!(
        hub.page(&reader, initial["snapshotId"].as_str().unwrap(), false, now)
            .unwrap()["records"][0]["value"],
        true
    );
    hub.subscribe(&reader, "reader", filter("test.facts.v1"), Some(1), now)
        .unwrap();
    let frame = hub.deliveries(now).pop().unwrap().frame;
    assert_eq!(frame["fromRevision"], "1");
    assert_eq!(frame["toRevision"], "2");
    assert_eq!(frame["changes"][0]["reason"], "sourceStopped");
    ack(&hub, &reader, &frame, now);
}

#[test]
fn core_presentation_identity_cannot_be_overwritten_by_plugin_contribution() {
    let (hub, writer, reader, now) = fixture();
    let core = Owner {
        context: writer.context.clone(),
        producer: Producer::Core,
        target: DeliveryTarget::Process {
            instance_id: "core".into(),
            view_id: "host".into(),
        },
    };
    hub.put(
        &core,
        "workspace.presentation.v1",
        "current",
        0,
        json!({"activeEditorViewId":"terminal"}),
        1000,
        now,
    )
    .unwrap();
    hub.put(
        &writer,
        "workspace.presentation.v1",
        "current",
        0,
        json!({"activeEditorViewId":"fake"}),
        1001,
        now,
    )
    .unwrap();
    assert_eq!(
        hub.get(
            &reader,
            "workspace.presentation.v1",
            "current",
            Some(&Producer::Core),
            now
        )
        .unwrap()["record"]["value"]["activeEditorViewId"],
        "terminal"
    );
    assert_eq!(
        hub.list(&reader, filter("workspace.presentation.v1"), now)
            .unwrap()["records"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}
