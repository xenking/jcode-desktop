use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::sections::{ReadWindow, SECTION_LIST_LIMIT, read_hosts, section_get};

fn timestamp<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<DateTime<Utc>>, D::Error> {
    Ok(crate::inbox::timestamp(
        Option::<String>::deserialize(deserializer)?.as_deref(),
    ))
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    pub qitem_id: String,
    pub destination_session: Option<String>,
    pub source_session: Option<String>,
    pub state: Option<String>,
    pub priority: Option<String>,
    #[serde(default, deserialize_with = "timestamp")]
    pub ts_created: Option<DateTime<Utc>>,
    pub summary: Option<String>,
    pub body: Option<String>,
    pub closure_reason: Option<String>,
    pub blocked_on: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Workflow {
    pub instance_id: String,
    pub workflow_name: String,
    pub workflow_version: Option<String>,
    pub status: Option<String>,
    pub current_step_id: Option<String>,
    #[serde(default, deserialize_with = "timestamp")]
    pub created_at: Option<DateTime<Utc>>,
    pub created_by_session: Option<String>,
    pub current_frontier: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostWork {
    pub host: String,
    pub queue: Result<ReadWindow<QueueItem>, String>,
    pub workflows: Result<ReadWindow<Workflow>, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Work {
    pub hosts: Vec<HostWork>,
    pub discovery_issue: Option<String>,
}

pub fn fetch_work(base: &str) -> Work {
    let (hosts, discovery_issue) = read_hosts(base, |host| {
        let limit = SECTION_LIST_LIMIT + 1;
        let (queue, workflows) = std::thread::scope(|scope| {
            let queue = scope.spawn(|| {
                section_get::<Vec<QueueItem>>(
                    base,
                    host,
                    &format!("/api/queue/list?limit={limit}&sort=updated"),
                )
                .map(Into::into)
            });
            let workflows = section_get::<Vec<Workflow>>(
                base,
                host,
                &format!("/api/workflow/list?limit={limit}"),
            )
            .map(Into::into);
            (queue.join().expect("queue reader panicked"), workflows)
        });
        HostWork {
            host: host.into(),
            queue,
            workflows,
        }
    });
    Work {
        hosts,
        discovery_issue,
    }
}

impl Work {
    pub fn fixture() -> Self {
        // The live lists were empty. This separate example exercises inspector and filter states.
        Self {
            hosts: vec![HostWork {
                host: "local".into(),
                queue: Ok(serde_json::from_str::<Vec<_>>(include_str!(
                    "../tests/fixtures/work-queue-example.json"
                ))
                .unwrap()
                .into()),
                workflows: Ok(serde_json::from_str::<Vec<_>>(include_str!(
                    "../tests/fixtures/work-workflow-example.json"
                ))
                .unwrap()
                .into()),
            }],
            discovery_issue: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_fixture_keeps_owner_current_step_and_closure_reason() {
        let work = Work::fixture();
        let host = &work.hosts[0];
        let items = &host.queue.as_ref().unwrap().items;
        assert_eq!(
            items[0].destination_session.as_deref(),
            Some("operator@kernel")
        );
        assert_eq!(items[0].closure_reason, None);
        assert_eq!(items[1].closure_reason.as_deref(), Some("completed"));
        assert_eq!(items[1].priority, None);
        assert_eq!(
            host.workflows.as_ref().unwrap().items[0]
                .current_step_id
                .as_deref(),
            Some("verify")
        );
        let window = ReadWindow::from(vec![&items[0]; SECTION_LIST_LIMIT + 1]);
        assert_eq!(window.items.len(), SECTION_LIST_LIMIT);
        assert!(window.truncated);
    }

    #[test]
    fn work_live_capture_distinguishes_empty_and_refused() {
        let (base, server) = crate::sections::serve_fixture(5, |target| match target {
            "/api/hosts" => (200, include_str!("../tests/fixtures/work-hosts-get.json")),
            "/api/queue/list?limit=101&sort=updated" => {
                (200, include_str!("../tests/fixtures/work-queue-get.json"))
            }
            "/api/workflow/list?limit=101" => (
                200,
                include_str!("../tests/fixtures/work-workflow-get.json"),
            ),
            "/api/queue/list?limit=101&sort=updated&host=ai-core" => (
                403,
                include_str!("../tests/fixtures/work-queue-ai-core-get.json"),
            ),
            "/api/workflow/list?limit=101&host=ai-core" => (
                403,
                include_str!("../tests/fixtures/work-workflow-ai-core-get.json"),
            ),
            _ => panic!("unexpected route {target}"),
        });
        let work = fetch_work(&base);
        server.join().unwrap();
        assert!(work.discovery_issue.is_none());
        assert!(work.hosts[0].queue.as_ref().unwrap().items.is_empty());
        assert!(work.hosts[0].workflows.as_ref().unwrap().items.is_empty());
        assert_eq!(work.hosts[1].host, "ai-core");
        assert_eq!(work.hosts[1].queue, Err("HTTP 403".into()));
        assert_eq!(work.hosts[1].workflows, Err("HTTP 403".into()));
    }

    #[test]
    #[ignore = "requires a running OpenRig daemon"]
    fn live_work_reads_local_and_reports_each_remote_route() {
        let base = std::env::var("OPENRIG_LIVE_BASE").expect("OPENRIG_LIVE_BASE");
        let work = fetch_work(&base);
        assert!(work.discovery_issue.is_none(), "{:?}", work.discovery_issue);
        let local = &work.hosts[0];
        assert_eq!(local.host, "local");
        assert!(local.queue.is_ok() && local.workflows.is_ok(), "{local:?}");
        for host in work.hosts {
            println!(
                "{}: queue={:?}, workflows={:?}",
                host.host,
                host.queue.as_ref().map(|rows| rows.items.len()),
                host.workflows.as_ref().map(|rows| rows.items.len())
            );
        }
    }
}
