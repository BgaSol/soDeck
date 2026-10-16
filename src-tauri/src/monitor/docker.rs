use super::*;
use crate::{docker as query, traffic::TrafficPurpose};

fn summarize(containers: &[query::Container], health: AppResult<Vec<String>>) -> DockerSummary {
    let mut summary = DockerSummary {
        total: containers.len(),
        ..Default::default()
    };
    for c in containers {
        *summary.by_state.entry(c.state.clone()).or_default() += 1;
    }
    match health {
        Ok(ids) => {
            let ids: std::collections::HashSet<_> = ids.into_iter().collect();
            summary.unhealthy = Some(containers.iter().filter(|c| ids.contains(&c.id)).count());
        }
        Err(e) => summary.health_error = Some(e.message),
    }
    summary
}

pub(super) fn run(state: Weak<State>, entry: Arc<Entry>) {
    let mut connection = None;
    let mut next = Instant::now();
    let mut completed_at = Instant::now();
    let mut last_refresh = 0;
    let mut images_refresh = u64::MAX;
    let mut failures = 0u32;
    let mut was_visible = false;
    let mut requested_before = false;
    let mut auth_paused = false;
    loop {
        let Some(state) = state.upgrade() else { break };
        if entry.check(&state).is_err() {
            break;
        }
        let interests = entry.interests(&state);
        let requested = interests
            .iter()
            .any(|i| i.docker.summary || i.docker.containers || i.docker.images);
        let visible = interests
            .iter()
            .any(|i| i.visible && (i.docker.summary || i.docker.containers || i.docker.images));
        let want_images = interests.iter().any(|i| i.visible && i.docker.images);
        let refresh = entry.core.lock().unwrap().docker_refresh;
        let forced = refresh != last_refresh;
        if forced {
            auth_paused = false;
        }
        if !requested {
            connection = None;
        }
        if !visible && was_visible && failures == 0 {
            next = completed_at + Duration::from_secs(60);
        }
        let due = requested
            && !auth_paused
            && (forced || !requested_before || (visible && !was_visible) || Instant::now() >= next);
        was_visible = visible;
        requested_before = requested;
        if due {
            last_refresh = refresh;
            entry.core.lock().unwrap().docker.refreshing = true;
            entry.publish_docker(&state);
            let engine = (|| {
                if connection.is_none() {
                    connection = Some(connection::connect_verified(
                        &*entry.spec()?,
                        &entry.cancel,
                        TrafficPurpose::DockerMonitor,
                    )?);
                }
                query::detect_on(connection.as_ref().unwrap().session(), &|| {
                    entry.check(&state)
                })
            })();
            if entry.check(&state).is_err() {
                break;
            }
            let at = now();
            match engine {
                Ok(engine) => {
                    let running = engine.state == "running";
                    let endpoint = engine.endpoint.clone();
                    let generation = {
                        let mut c = entry.core.lock().unwrap();
                        if c.docker.engine.value.as_ref().map(|s| &s.endpoint) != Some(&endpoint) {
                            c.docker.engine_generation += 1;
                            c.docker.containers = Metric::default();
                            c.docker.summary = Metric::default();
                            c.docker.images = Metric::default();
                            c.images_next = Instant::now();
                            c.images_paused = false;
                            images_refresh = u64::MAX;
                        }
                        c.docker.engine = Metric::success(engine, at);
                        if !running {
                            c.docker
                                .containers
                                .fail("unavailable", "Docker 引擎当前不可用");
                            c.docker
                                .summary
                                .fail("unavailable", "Docker 引擎当前不可用");
                            c.docker.images.fail("unavailable", "Docker 引擎当前不可用");
                        }
                        c.docker.engine_generation
                    };
                    if running {
                        let session = connection.as_ref().unwrap().session();
                        let result =
                            query::containers_on(session, &endpoint, &|| entry.check(&state));
                        let result = result.map(|containers| {
                            let summary = summarize(
                                &containers,
                                query::unhealthy_on(session, &endpoint, &|| entry.check(&state)),
                            );
                            (containers, summary)
                        });
                        let mut c = entry.core.lock().unwrap();
                        if c.docker.engine_generation == generation {
                            match result {
                                Ok((containers, summary)) => {
                                    c.docker.containers = Metric::success(containers, now());
                                    c.docker.summary = Metric::success(summary, now());
                                    failures = 0;
                                }
                                Err(e) => {
                                    c.docker.containers.fail("stale", &e.message);
                                    c.docker.summary.fail("stale", &e.message);
                                    connection = None;
                                    failures = failures.saturating_add(1);
                                }
                            }
                        }
                    } else {
                        failures = failures.saturating_add(1);
                    }
                }
                Err(e) => {
                    let mut c = entry.core.lock().unwrap();
                    c.docker.engine.fail("stale", &e.message);
                    c.docker.containers.fail("stale", &e.message);
                    c.docker.summary.fail("stale", &e.message);
                    c.docker.images.fail("stale", &e.message);
                    auth_paused =
                        matches!(e.kind.as_str(), "hostKey" | "authentication" | "permission");
                    failures = failures.saturating_add(1);
                    connection = None;
                }
            }
            entry.core.lock().unwrap().docker.refreshing = false;
            entry.publish_docker(&state);
            let seconds = if failures > 0 {
                (30u64.saturating_mul(2u64.pow(failures.min(5) - 1))).min(300)
            } else if visible {
                15
            } else {
                60
            };
            completed_at = Instant::now();
            next = completed_at + Duration::from_secs(seconds);
        }
        if want_images {
            let request = {
                let mut c = entry.core.lock().unwrap();
                let ready = c.docker.engine.status == "ready"
                    && c.docker
                        .engine
                        .value
                        .as_ref()
                        .is_some_and(|s| s.state == "running");
                let old = Instant::now() >= c.images_next;
                if ready
                    && !c.images_running
                    && (!c.images_paused || images_refresh != refresh)
                    && (old || images_refresh != refresh)
                {
                    c.images_running = true;
                    images_refresh = refresh;
                    Some((
                        c.docker.engine_generation,
                        c.docker.engine.value.as_ref().unwrap().endpoint.clone(),
                    ))
                } else {
                    None
                }
            };
            if let Some((generation, endpoint)) = request {
                let e = entry.clone();
                let s = state.clone();
                spawn(&entry, move || {
                    let result = (|| {
                        let connection = connection::connect_verified(
                            &*e.spec()?,
                            &e.cancel,
                            TrafficPurpose::DockerMonitor,
                        )?;
                        query::images_on(connection.session(), &endpoint, &|| e.check(&s))
                    })();
                    {
                        let mut c = e.core.lock().unwrap();
                        c.images_running = false;
                        if c.docker.engine_generation == generation {
                            c.images_next = Instant::now() + Duration::from_secs(60);
                            match result {
                                Ok(images) => {
                                    c.images_paused = false;
                                    c.docker.images = Metric::success(images, now());
                                }
                                Err(error) => {
                                    c.images_paused = matches!(
                                        error.kind.as_str(),
                                        "hostKey" | "authentication" | "permission"
                                    );
                                    c.docker.images.fail("stale", &error.message);
                                    // Retry time is independent of the last successful data timestamp.
                                }
                            }
                        }
                    }
                    if e.check(&s).is_ok() {
                        e.publish_docker(&s);
                    }
                });
            }
        }
        drop(state);
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stopped_is_not_unhealthy_and_health_failure_is_unknown() {
        let containers = vec![query::Container {
            id: "a".into(),
            name: "a".into(),
            image: "i".into(),
            state: "exited".into(),
            status: "Exited".into(),
        }];
        let s = summarize(&containers, Ok(vec!["removed".into()]));
        assert_eq!(s.by_state["exited"], 1);
        assert_eq!(s.unhealthy, Some(0));
        let s = summarize(
            &containers,
            Err(AppError::new("docker", "unsupported filter")),
        );
        assert_eq!(s.unhealthy, None);
        assert!(s.health_error.is_some());
    }
}
