use serde_json::{json, Value};

pub fn output_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["what", "entry_symbol", "key_symbols", "look_here_for", "subdir_notes"],
        "properties": {
            "what": {
                "type": "string",
                "description": "One or two sentences on this directory's responsibility."
            },
            "entry_symbol": {
                "type": ["string", "null"],
                "description": "The name of the symbol a reader should start from. Must be copied exactly from the provided symbol list, or null if none fits."
            },
            "key_symbols": {
                "type": "array",
                "description": "The handful of symbols worth knowing. Use only names from the provided symbol list.",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["name", "description"],
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Exact symbol name copied from the provided list."
                        },
                        "description": {
                            "type": "string",
                            "description": "Short phrase describing what this symbol does."
                        }
                    }
                }
            },
            "look_here_for": {
                "type": "array",
                "description": "Query terms that should route a reader to this directory.",
                "items": { "type": "string" }
            },
            "subdir_notes": {
                "type": "array",
                "description": "One note per subdirectory, using the provided subdirectory names.",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["name", "note"],
                    "properties": {
                        "name": { "type": "string" },
                        "note": { "type": "string" }
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_forbids_additional_properties_everywhere() {
        fn check(v: &Value) {
            if let Some(obj) = v.as_object() {
                if obj.get("type").and_then(Value::as_str) == Some("object") {
                    assert_eq!(
                        obj.get("additionalProperties"),
                        Some(&json!(false)),
                        "object schema must set additionalProperties:false — {v}"
                    );
                    assert!(
                        obj.contains_key("required"),
                        "object schema must list required — {v}"
                    );
                }
                for nested in obj.values() {
                    check(nested);
                }
            } else if let Some(arr) = v.as_array() {
                for nested in arr {
                    check(nested);
                }
            }
        }
        check(&output_schema());
    }

    #[test]
    fn schema_uses_no_unsupported_keywords() {
        let text = output_schema().to_string();
        for bad in ["minLength", "maxLength", "minimum", "maximum", "multipleOf"] {
            assert!(!text.contains(bad), "schema must not use {bad}");
        }
    }

    #[test]
    fn schema_has_no_line_or_file_fields() {
        // The model must never be able to emit an anchor.
        let text = output_schema().to_string();
        assert!(!text.contains("\"line\""), "model must not emit line numbers");
        assert!(!text.contains("\"file\""), "model must not emit file paths");
    }
}
