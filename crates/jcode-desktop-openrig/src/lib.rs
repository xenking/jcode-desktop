#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SectionKind { Topology, Specs, Scopes, Terminals, Needs, System, Config, Connections, Usage, UsageHosts, Cost }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row { pub id: String, pub depth: u8, pub label: String, pub status: Option<String>, pub detail: Vec<(String, String)>, pub children_count: usize }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section { pub kind: SectionKind, pub title: String, pub rows: Vec<Row> }
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot { pub sections: Vec<Section>, pub error: Option<String> }

pub fn default_base_url() -> String { std::env::var("OPENRIG_URL").unwrap_or_else(|_| "http://127.0.0.1:7433".into()) }
pub fn fetch_snapshot(_base_url: &str) -> Snapshot { Snapshot { error: Some("OpenRig client not yet connected".into()), ..Snapshot::default() } }
impl Snapshot {
    pub fn fixture() -> Self {
        let kinds = [(SectionKind::Topology,"Topology"),(SectionKind::Specs,"Specs"),(SectionKind::Scopes,"Scopes"),(SectionKind::Terminals,"Terminals"),(SectionKind::Needs,"Needs attention"),(SectionKind::System,"System"),(SectionKind::Config,"Config"),(SectionKind::Connections,"Connections"),(SectionKind::Usage,"Usage"),(SectionKind::UsageHosts,"Usage hosts"),(SectionKind::Cost,"Cost")];
        Self { sections: kinds.into_iter().map(|(kind,title)| Section { kind, title: title.into(), rows: vec![Row { id: title.into(), depth: 0, label: format!("Demo {title}"), status: Some("ready".into()), detail: vec![("Source".into(), "Demo".into())], children_count: 0 }] }).collect(), error: None }
    }
}
