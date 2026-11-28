use std::collections::BTreeMap;

use serde::Deserialize;

use crate::sections::{ReadWindow, SECTION_LIST_LIMIT, host_target, read_hosts, section_get};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Rig,
    Agent,
    Workflow,
    Application,
    ContextPack,
    AgentImage,
    Plugin,
    Skill,
    Other,
}

impl Kind {
    pub const ALL: [Self; 9] = [
        Self::Rig,
        Self::Agent,
        Self::Workflow,
        Self::Application,
        Self::ContextPack,
        Self::AgentImage,
        Self::Plugin,
        Self::Skill,
        Self::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Rig => "Rig specs",
            Self::Agent => "Agent specs",
            Self::Workflow => "Workflow specs",
            Self::Application => "Applications",
            Self::ContextPack => "Context packs",
            Self::AgentImage => "Agent images",
            Self::Plugin => "Plugins",
            Self::Skill => "Skills",
            Self::Other => "Other specs",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub version: Option<String>,
    pub summary: Option<String>,
    pub source: Option<String>,
    pub runtimes: Vec<String>,
    pub files: Option<ReadWindow<String>>,
    pub skill_count: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Catalog {
    Specs,
    ContextPacks,
    AgentImages,
    Plugins,
    Skills,
}

impl Catalog {
    const ALL: [Self; 5] = [
        Self::Specs,
        Self::ContextPacks,
        Self::AgentImages,
        Self::Plugins,
        Self::Skills,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Specs => "Specs",
            Self::ContextPacks => "Context packs",
            Self::AgentImages => "Agent images",
            Self::Plugins => "Plugins",
            Self::Skills => "Skills",
        }
    }

    pub fn includes(self, kind: Kind) -> bool {
        match self {
            Self::Specs => matches!(
                kind,
                Kind::Rig | Kind::Agent | Kind::Workflow | Kind::Application | Kind::Other
            ),
            Self::ContextPacks => kind == Kind::ContextPack,
            Self::AgentImages => kind == Kind::AgentImage,
            Self::Plugins => kind == Kind::Plugin,
            Self::Skills => kind == Kind::Skill,
        }
    }

    fn route(self) -> &'static str {
        match self {
            Self::Specs => "/api/specs/library",
            Self::ContextPacks => "/api/context-packs/library",
            Self::AgentImages => "/api/agent-images/library",
            Self::Plugins => "/api/plugins",
            Self::Skills => "/api/skills/library",
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogEntry {
    id: String,
    name: String,
    kind: Option<String>,
    has_services: Option<bool>,
    version: Option<String>,
    summary: Option<String>,
    description: Option<String>,
    notes: Option<String>,
    purpose: Option<String>,
    source_type: Option<String>,
    source_label: Option<String>,
    source: Option<String>,
    runtime: Option<String>,
    runtimes: Option<Vec<String>>,
    files: Option<Vec<FileEntry>>,
    skill_count: Option<u64>,
}

#[derive(Deserialize)]
struct FileEntry {
    path: String,
}

fn catalog_rows(catalog: Catalog, rows: Vec<CatalogEntry>) -> ReadWindow<Entry> {
    let rows: Vec<_> = rows
        .into_iter()
        .map(|row| {
            let kind = match catalog {
                Catalog::Specs => match row.kind.as_deref() {
                    Some("rig") if row.has_services == Some(true) => Kind::Application,
                    Some("rig") => Kind::Rig,
                    Some("agent") => Kind::Agent,
                    Some("workflow") => Kind::Workflow,
                    _ => Kind::Other,
                },
                Catalog::ContextPacks => Kind::ContextPack,
                Catalog::AgentImages => Kind::AgentImage,
                Catalog::Plugins => Kind::Plugin,
                Catalog::Skills => Kind::Skill,
            };
            Entry {
                id: row.id,
                name: row.name,
                kind,
                version: row.version,
                summary: row
                    .summary
                    .or(row.description)
                    .or(row.notes)
                    .or(row.purpose),
                source: row.source_type.or(row.source_label).or(row.source),
                runtimes: row
                    .runtimes
                    .unwrap_or_else(|| row.runtime.into_iter().collect()),
                files: row.files.map(|files| {
                    ReadWindow::from(files.into_iter().map(|file| file.path).collect::<Vec<_>>())
                }),
                skill_count: row.skill_count,
            }
        })
        .collect();
    rows.into()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostBuild {
    pub host: String,
    pub catalogs: Vec<(Catalog, Result<ReadWindow<Entry>, String>)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Build {
    pub hosts: Vec<HostBuild>,
    pub discovery_issue: Option<String>,
}

pub fn fetch_build(base: &str) -> Build {
    let (hosts, discovery_issue) = read_hosts(base, |host| {
        let catalogs = std::thread::scope(|scope| {
            Catalog::ALL
                .into_iter()
                .map(|catalog| {
                    (
                        catalog,
                        scope.spawn(move || {
                            section_get(host, catalog.route())
                                .map(|rows| catalog_rows(catalog, rows))
                        }),
                    )
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|(catalog, task)| (catalog, task.join().expect("catalog reader panicked")))
                .collect()
        });
        HostBuild {
            host: host.alias().into(),
            catalogs,
        }
    });
    Build {
        hosts,
        discovery_issue,
    }
}

const FIXTURE_CATALOGS: [&str; 5] = [
    include_str!("../tests/fixtures/build-specs-get.json"),
    include_str!("../tests/fixtures/build-packs-get.json"),
    include_str!("../tests/fixtures/build-images-get.json"),
    include_str!("../tests/fixtures/build-plugins-get.json"),
    include_str!("../tests/fixtures/build-skills-get.json"),
];

impl Build {
    pub fn fixture() -> Self {
        Self {
            hosts: vec![HostBuild {
                host: "local".into(),
                catalogs: Catalog::ALL
                    .into_iter()
                    .zip(FIXTURE_CATALOGS)
                    .map(|(catalog, json)| {
                        (
                            catalog,
                            Ok(catalog_rows(catalog, serde_json::from_str(json).unwrap())),
                        )
                    })
                    .collect(),
            }],
            discovery_issue: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    pub runtime: Option<String>,
    pub model: Option<String>,
    pub agent_ref: Option<String>,
    pub profile: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub step_id: String,
    pub role: Option<String>,
    pub objective: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Preview {
    Rig(ReadWindow<Member>),
    Agent(ReadWindow<Member>),
    Workflow(ReadWindow<Step>),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RigMember {
    id: String,
    runtime: Option<String>,
    model: Option<String>,
    agent_ref: Option<String>,
    profile: Option<String>,
}

#[derive(Deserialize)]
struct Pod {
    id: String,
    members: Vec<RigMember>,
}

#[derive(Deserialize)]
struct AgentProfile {
    name: String,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum Review {
    Rig {
        pods: Option<Vec<Pod>>,
        nodes: Option<Vec<RigMember>>,
        raw: Option<String>,
    },
    Agent {
        profiles: Vec<AgentProfile>,
        raw: Option<String>,
    },
    Workflow {
        steps: Vec<Step>,
    },
}

#[derive(Default, Deserialize)]
struct Choices {
    runtime: Option<String>,
    model: Option<String>,
}

#[derive(Deserialize)]
struct DeclaredPod {
    id: String,
    members: Vec<DeclaredMember>,
}

#[derive(Deserialize)]
struct DeclaredMember {
    id: String,
    model: Option<String>,
}

#[derive(Deserialize)]
struct DeclaredProfile {
    preferences: Option<Choices>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Declarations {
    pods: Vec<DeclaredPod>,
    defaults: Option<Choices>,
    profiles: BTreeMap<String, Option<DeclaredProfile>>,
}

fn declarations(raw: Option<&str>) -> Declarations {
    // The review projection omits pod member pins and profile preferences; the same GET includes their authored YAML.
    raw.and_then(|raw| serde_yaml::from_str(raw).ok())
        .unwrap_or_default()
}

fn preview(review: Review) -> Result<Preview, String> {
    Ok(match review {
        Review::Rig { pods, nodes, raw } => {
            let mut members = Vec::new();
            if let Some(pods) = pods {
                let declared = declarations(raw.as_deref());
                for pod in pods {
                    let declared_pod = declared.pods.iter().find(|item| item.id == pod.id);
                    for member in pod.members {
                        let model = declared_pod
                            .and_then(|pod| pod.members.iter().find(|item| item.id == member.id))
                            .and_then(|item| item.model.clone());
                        members.push(Member {
                            name: format!("{}.{}", pod.id, member.id),
                            runtime: member.runtime,
                            model,
                            agent_ref: member.agent_ref,
                            profile: member.profile,
                        });
                        if members.len() > SECTION_LIST_LIMIT {
                            break;
                        }
                    }
                    if members.len() > SECTION_LIST_LIMIT {
                        break;
                    }
                }
            } else if let Some(nodes) = nodes {
                members = nodes
                    .into_iter()
                    .take(SECTION_LIST_LIMIT + 1)
                    .map(|member| Member {
                        name: member.id,
                        runtime: member.runtime,
                        model: member.model,
                        agent_ref: None,
                        profile: None,
                    })
                    .collect();
            } else {
                return Err("Spec review has no member projection".into());
            }
            Preview::Rig(members.into())
        }
        Review::Agent { profiles, raw } => {
            let declared = declarations(raw.as_deref());
            let mut members = Vec::new();
            if let Some(defaults) = declared.defaults {
                members.push(Member {
                    name: "Defaults".into(),
                    runtime: defaults.runtime,
                    model: defaults.model,
                    agent_ref: None,
                    profile: None,
                });
            }
            for profile in profiles.into_iter().take(SECTION_LIST_LIMIT + 1) {
                let choices = declared
                    .profiles
                    .get(&profile.name)
                    .and_then(Option::as_ref)
                    .and_then(|profile| profile.preferences.as_ref());
                members.push(Member {
                    name: profile.name,
                    runtime: choices.and_then(|fields| fields.runtime.clone()),
                    model: choices.and_then(|fields| fields.model.clone()),
                    agent_ref: None,
                    profile: None,
                });
            }
            Preview::Agent(members.into())
        }
        Review::Workflow { steps } => Preview::Workflow(steps.into()),
    })
}

pub fn fetch_preview(base: &str, host: &str, id: &str) -> Result<Preview, String> {
    section_get(
        &host_target(base, host)?,
        &format!("/api/specs/library/{}/review", crate::encode(id)),
    )
    .and_then(preview)
}

pub fn fixture_preview(id: &str) -> Result<Preview, String> {
    let json = match id {
        "0a25241dadbcaf2d" => include_str!("../tests/fixtures/build-rig-review-get.json"),
        "33d11aa6749be7f8" => include_str!("../tests/fixtures/build-agent-review-get.json"),
        "workflow:basic-loop:1" => include_str!("../tests/fixtures/build-workflow-review-get.json"),
        _ => return Err("No captured preview for this fixture".into()),
    };
    preview(serde_json::from_str(json).map_err(|error| error.to_string())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_loads_remote_catalogs_and_preview_directly() {
        let catalog = |route: &str| {
            let (_, body) = Catalog::ALL
                .into_iter()
                .zip(FIXTURE_CATALOGS)
                .find(|(catalog, _)| catalog.route() == route)
                .unwrap_or_else(|| panic!("unexpected route {route}"));
            (200, body.to_owned())
        };
        let (remote, remote_server) = crate::sections::serve_fixture(6, move |route| {
            if route == "/api/specs/library/0a25241dadbcaf2d/review" {
                return (
                    200,
                    include_str!("../tests/fixtures/build-rig-review-get.json").into(),
                );
            }
            catalog(route)
        });
        let (base, server) = crate::sections::serve_fixture(7, move |route| {
            if route == "/api/hosts" {
                return (
                    200,
                    serde_json::json!({"hosts":[{"id":"ai-core", "url":remote}]}).to_string(),
                );
            }
            catalog(route)
        });
        let build = fetch_build(&base);
        let Preview::Rig(members) = fetch_preview(&base, "ai-core", "0a25241dadbcaf2d").unwrap()
        else {
            panic!()
        };
        server.join().unwrap();
        remote_server.join().unwrap();
        assert_eq!(members.items.len(), 3);
        assert_eq!(members.items[0].runtime.as_deref(), Some("claude-code"));
        assert_eq!(members.items[0].model, None);
        let local = &build.hosts[0];
        assert!(local.catalogs.iter().all(|(_, read)| read.is_ok()));
        assert!(
            local.catalogs[0]
                .1
                .as_ref()
                .unwrap()
                .items
                .iter()
                .any(|row| row.kind == Kind::Application)
        );
        assert_eq!(build.hosts[1].host, "ai-core");
        assert!(build.hosts[1].catalogs.iter().all(|(_, read)| read.is_ok()));
        assert!(matches!(
            fixture_preview("33d11aa6749be7f8"),
            Ok(Preview::Agent(_))
        ));
        assert!(matches!(
            fixture_preview("workflow:basic-loop:1"),
            Ok(Preview::Workflow(_))
        ));
    }

    #[test]
    fn authored_member_pins_are_not_guessed_from_runtime() {
        let raw = "pods: [{id: review, members: [{id: author, model: explicit}, {id: critic}]}]";
        let review = serde_json::json!({"kind":"rig","raw":raw,"pods":[{"id":"review","members":[{"id":"author","runtime":"jcode"},{"id":"critic","runtime":"jcode"}]}]});
        let Preview::Rig(window) = preview(serde_json::from_value(review).unwrap()).unwrap() else {
            panic!()
        };
        assert_eq!(window.items[0].model.as_deref(), Some("explicit"));
        assert_eq!(window.items[1].model, None);
    }

    #[test]
    #[ignore = "requires a running OpenRig daemon"]
    fn live_build_reads_catalogs_and_spec_preview() {
        let base = std::env::var("OPENRIG_LIVE_BASE").expect("OPENRIG_LIVE_BASE");
        let build = fetch_build(&base);
        assert!(
            build.discovery_issue.is_none(),
            "{:?}",
            build.discovery_issue
        );
        for host in &build.hosts {
            for (catalog, rows) in &host.catalogs {
                println!(
                    "{} {}: {:?}",
                    host.host,
                    catalog.label(),
                    rows.as_ref().map(|rows| rows.items.len())
                );
                if host.host == "local" {
                    assert!(rows.is_ok(), "{catalog:?}: {rows:?}");
                }
                if *catalog == Catalog::Specs
                    && let Ok(rows) = rows
                {
                    for kind in [Kind::Rig, Kind::Agent, Kind::Workflow] {
                        if let Some(row) = rows.items.iter().find(|row| row.kind == kind) {
                            let result = fetch_preview(&base, &host.host, &row.id).unwrap();
                            assert!(matches!(
                                (kind, result),
                                (Kind::Rig, Preview::Rig(_))
                                    | (Kind::Agent, Preview::Agent(_))
                                    | (Kind::Workflow, Preview::Workflow(_))
                            ));
                            println!("{} {kind:?} preview: OK", host.host);
                        }
                    }
                }
            }
        }
    }
}
