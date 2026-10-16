use mathscape_config::chart::{DEDICATED_STREAM_FIELDS, REGENERATE, generated_schemas, stream_config_schema, values_schema};
use std::path::Path;

#[test]
fn committed_schemas_are_fresh() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (relative, rendered) in generated_schemas() {
        if let Err(stale) = helm_forge::values_schema::check_fresh(&root.join(relative), &rendered, REGENERATE) {
            panic!("{stale}");
        }
    }
}

#[test]
fn stream_config_is_typed_without_its_dedicated_fields() {
    let schema = values_schema();
    let config = &schema["definitions"]["StreamConfig"];
    assert_eq!(config["additionalProperties"], false);
    for field in DEDICATED_STREAM_FIELDS {
        assert!(config["properties"].get(field).is_none(), "{field}");
    }
    assert!(config.get("required").is_none());
    assert_eq!(schema["definitions"]["StreamMechanism"]["additionalProperties"], false);
    assert_eq!(schema["additionalProperties"], false);
}

#[test]
fn the_standalone_stream_config_schema_carries_no_defaults() {
    let schema = stream_config_schema();
    assert_eq!(schema["additionalProperties"], false);
    for field in DEDICATED_STREAM_FIELDS {
        assert!(schema["properties"].get(field).is_none(), "{field}");
    }
    assert!(!serde_json::to_string(&schema).unwrap().contains("\"default\""));
}
