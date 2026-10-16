use schemars::JsonSchema;
use schemars::r#gen::SchemaGenerator;
use schemars::schema::Schema;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const VALUES_SCHEMA_PATH: &str = "deploy/charts/mathscape/values.schema.json";
pub const STREAM_CONFIG_SCHEMA_PATH: &str = "deploy/schemas/stream-config.schema.json";
pub const REGENERATE: &str = "cargo run -p mathscape-config --bin mathscape-values-schema";
pub const DEDICATED_STREAM_FIELDS: [&str; 4] = ["seed", "data_dir", "until_epoch", "health_addr"];

type Object = BTreeMap<String, Value>;

#[derive(JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChartValues {
    pub image: Option<Image>,
    pub image_pull_secrets: Option<Vec<Value>>,
    pub name_override: Option<String>,
    pub fullname_override: Option<String>,
    pub replica_count: Option<u32>,
    pub ports: Option<Vec<ContainerPort>>,
    pub service: Option<Service>,
    pub env: Option<Vec<Value>>,
    pub engine: Option<Toggle>,
    pub stream: Option<Stream>,
    pub env_from: Option<Vec<Value>>,
    pub resources: Option<Object>,
    pub persistence: Option<Persistence>,
    pub health: Option<Health>,
    pub monitoring: Option<Monitoring>,
    pub network_policy: Option<NetworkPolicy>,
    pub pod_security_context: Option<Object>,
    pub security_context: Option<Object>,
    pub service_account: Option<ServiceAccount>,
    pub pdb: Option<Pdb>,
    pub termination_grace_period_seconds: Option<u32>,
    pub pod_annotations: Option<BTreeMap<String, String>>,
    pub node_selector: Option<BTreeMap<String, String>>,
    pub tolerations: Option<Vec<Value>>,
    pub affinity: Option<Object>,
    pub topology_spread_constraints: Option<Vec<Value>>,
    pub priority_class_name: Option<String>,
    pub command: Option<Vec<String>>,
    pub args: Option<Vec<String>>,
    pub additional_labels: Option<BTreeMap<String, String>>,
    pub annotations: Option<BTreeMap<String, String>>,
    pub config_maps: Option<Vec<Value>>,
    pub downward_api: Option<Toggle>,
    pub startup_probe: Option<StartupProbe>,
    pub istio: Option<Toggle>,
    pub attestation: Option<Toggle>,
    pub lifecycle: Option<Object>,
    pub volume_mounts: Option<Vec<Value>>,
    pub volumes: Option<Vec<Value>>,
    pub global: Option<Object>,
    #[serde(rename = "pleme-lib")]
    pub pleme_lib: Option<Object>,
}

#[derive(JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    pub repository: Option<String>,
    pub tag: Option<String>,
    pub pull_policy: Option<PullPolicy>,
}

#[derive(JsonSchema)]
pub enum PullPolicy {
    Always,
    IfNotPresent,
    Never,
}

#[derive(JsonSchema)]
pub enum Protocol {
    #[serde(rename = "TCP")]
    Tcp,
    #[serde(rename = "UDP")]
    Udp,
    #[serde(rename = "SCTP")]
    Sctp,
}

#[derive(JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContainerPort {
    pub name: String,
    pub container_port: u16,
    pub protocol: Option<Protocol>,
}

#[derive(JsonSchema)]
pub enum ServiceType {
    ClusterIP,
    NodePort,
    LoadBalancer,
}

#[derive(JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServicePort {
    pub name: String,
    pub port: u16,
    pub target_port: Option<String>,
    pub protocol: Option<Protocol>,
}

#[derive(JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Service {
    #[serde(rename = "type")]
    pub kind: Option<ServiceType>,
    pub ports: Option<Vec<ServicePort>>,
}

#[derive(JsonSchema)]
pub struct Toggle {
    pub enabled: Option<bool>,
}

#[derive(JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Persistence {
    pub enabled: Option<bool>,
    pub size: Option<String>,
    pub storage_class: Option<String>,
    pub mount_path: Option<String>,
    pub access_modes: Option<Vec<String>>,
}

#[derive(JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    pub path: Option<String>,
    pub ready_path: Option<String>,
    pub port: Option<String>,
    pub liveness_initial_delay: Option<u32>,
    pub liveness_period: Option<u32>,
    pub liveness_failure_threshold: Option<u32>,
    pub readiness_initial_delay: Option<u32>,
    pub readiness_period: Option<u32>,
    pub readiness_failure_threshold: Option<u32>,
}

#[derive(JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Monitoring {
    pub enabled: Option<bool>,
    pub port: Option<String>,
    pub path: Option<String>,
    pub interval: Option<String>,
    pub scrape_timeout: Option<String>,
}

#[derive(JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NetworkPolicy {
    pub enabled: Option<bool>,
    pub allow_prometheus: Option<bool>,
    pub allow_dns: Option<bool>,
    pub additional_ingress: Option<Vec<Value>>,
    pub additional_egress: Option<Vec<Value>>,
}

#[derive(JsonSchema)]
pub struct ServiceAccount {
    pub create: Option<bool>,
    pub annotations: Option<BTreeMap<String, String>>,
    pub name: Option<String>,
}

#[derive(JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Pdb {
    pub enabled: Option<bool>,
    pub min_available: Option<u32>,
}

#[derive(JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartupProbe {
    pub enabled: Option<bool>,
    pub path: Option<String>,
    pub port: Option<String>,
    pub initial_delay_seconds: Option<u32>,
    pub period_seconds: Option<u32>,
    pub failure_threshold: Option<u32>,
}

#[derive(JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Stream {
    pub enabled: Option<bool>,
    #[schemars(schema_with = "seed_schema")]
    pub seed: Option<String>,
    pub data_dir: Option<String>,
    #[schemars(schema_with = "until_epoch_schema")]
    pub until_epoch: Option<String>,
    pub image: Option<StreamImage>,
    pub command: Option<Vec<String>>,
    pub rust_log: Option<String>,
    pub config: Option<crate::StreamConfig>,
    pub health: Option<StreamHealth>,
    pub storage: Option<StreamStorage>,
    pub resources: Option<Object>,
    pub termination_grace_period_seconds: Option<u32>,
    pub node_selector: Option<BTreeMap<String, String>>,
    pub alerts: Option<StreamAlerts>,
    pub scrape: Option<StreamScrape>,
    pub breathe: Option<Object>,
}

#[derive(JsonSchema)]
pub struct StreamImage {
    pub repository: Option<String>,
    pub tag: Option<String>,
}

#[derive(JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StreamHealth {
    pub port: Option<u16>,
    pub bind_address: Option<String>,
    pub liveness_period: Option<u32>,
    pub liveness_failure_threshold: Option<u32>,
    pub readiness_period: Option<u32>,
    pub startup_failure_threshold: Option<u32>,
    pub startup_period: Option<u32>,
}

#[derive(JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum StorageKind {
    ClaimTemplate,
    HostPath,
}

#[derive(JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StreamStorage {
    pub kind: Option<StorageKind>,
    pub mount_path: Option<String>,
    pub size: Option<String>,
    pub storage_class: Option<String>,
    pub path: Option<String>,
}

#[derive(JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StreamAlerts {
    pub enabled: Option<bool>,
    pub stalled_minutes: Option<u32>,
    pub snapshot_stale_minutes: Option<u32>,
    pub plateau_hours: Option<u32>,
}

#[derive(JsonSchema)]
pub struct StreamScrape {
    pub enabled: Option<bool>,
    pub interval: Option<String>,
}

fn seed_schema(_: &mut SchemaGenerator) -> Schema {
    serde_json::from_value(json!({
        "type": "string",
        "pattern": "^([0-9]+)?$",
        "description": "Non-negative integer, as a string; required when stream.enabled"
    }))
    .expect("a literal schema parses")
}

fn until_epoch_schema(_: &mut SchemaGenerator) -> Schema {
    serde_json::from_value(json!({
        "anyOf": [
            { "type": "integer", "minimum": 0 },
            { "type": "string", "pattern": "^([0-9]+)?$" }
        ]
    }))
    .expect("a literal schema parses")
}

#[must_use]
pub fn values_schema() -> Value {
    let mut schema = helm_forge::values_schema::strict_schema_value::<ChartValues>();
    if let Some(stream_config) = helm_forge::values_schema::definition_mut(&mut schema, "StreamConfig") {
        helm_forge::values_schema::remove_properties(stream_config, &DEDICATED_STREAM_FIELDS);
    }
    if let Some(stream) = helm_forge::values_schema::definition_mut(&mut schema, "Stream") {
        stream.insert(
            "if".into(),
            json!({ "properties": { "enabled": { "const": true } }, "required": ["enabled"] }),
        );
        stream.insert(
            "then".into(),
            json!({ "required": ["seed"], "properties": { "seed": { "minLength": 1 } } }),
        );
    }
    schema
}

#[must_use]
pub fn render_values_schema() -> String {
    helm_forge::values_schema::render(&values_schema())
}

#[must_use]
pub fn stream_config_schema() -> Value {
    let mut schema = helm_forge::values_schema::strict_schema_value::<crate::StreamConfig>();
    if let Some(root) = schema.as_object_mut() {
        helm_forge::values_schema::remove_properties(root, &DEDICATED_STREAM_FIELDS);
    }
    strip_defaults(&mut schema);
    schema
}

fn strip_defaults(schema: &mut Value) {
    let Some(map) = schema.as_object_mut() else {
        return;
    };
    map.remove("default");
    for key in ["properties", "definitions"] {
        if let Some(Value::Object(children)) = map.get_mut(key) {
            children.values_mut().for_each(strip_defaults);
        }
    }
    for key in ["allOf", "anyOf", "oneOf"] {
        if let Some(Value::Array(children)) = map.get_mut(key) {
            children.iter_mut().for_each(strip_defaults);
        }
    }
    for key in ["items", "additionalProperties"] {
        if let Some(child) = map.get_mut(key) {
            strip_defaults(child);
        }
    }
}

#[must_use]
pub fn generated_schemas() -> [(&'static str, String); 2] {
    [
        (VALUES_SCHEMA_PATH, render_values_schema()),
        (
            STREAM_CONFIG_SCHEMA_PATH,
            helm_forge::values_schema::render(&stream_config_schema()),
        ),
    ]
}
