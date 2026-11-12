use super::{DescribeRequest, DescribeResponse, Llm};
use crate::llm::schema::output_schema;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};

const API_BASE: &str = "https://api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2023-06-01";

pub struct HttpLlm {
    pub api_key: String,
    pub model: String,
    pub effort: Option<String>,
    pub use_batch: bool,
}

pub fn build_prompt(req: &DescribeRequest) -> String {
    let mut p = String::new();
    p.push_str(
        "You are documenting one directory of a code repository so that a coding agent can \
         navigate the repository without reading every file.\n\n",
    );
    p.push_str(&format!("Directory: {}\n\n", req.dir_path));

    p.push_str("Files in this directory:\n");
    for f in &req.file_names {
        p.push_str(&format!("  {f}\n"));
    }

    p.push_str("\nSymbols defined in this directory (copy these names exactly):\n");
    for s in &req.symbol_names {
        p.push_str(&format!("  {s}\n"));
    }

    if !req.child_summaries.is_empty() {
        p.push_str("\nSubdirectories and what each one does:\n");
        for (name, summary) in &req.child_summaries {
            p.push_str(&format!("  {name}: {summary}\n"));
        }
    }

    p.push_str(
        "\nRules:\n\
         - Use only symbol names from the list above, copied exactly. Never invent a name.\n\
         - Never write a file path or a line number. Those are added automatically.\n\
         - `what` is one or two sentences on this directory's responsibility.\n\
         - `key_symbols` is the handful worth knowing, not everything.\n\
         - `look_here_for` is the query terms that should route a reader here.\n",
    );
    p
}

pub fn build_message_body(model: &str, effort: Option<&str>, prompt: &str) -> Value {
    let mut output_config = json!({
        "format": {
            "type": "json_schema",
            "schema": output_schema()
        }
    });
    // `effort` is rejected on claude-haiku-4-5, so it is set only when supplied.
    if let Some(e) = effort {
        output_config["effort"] = json!(e);
    }

    json!({
        "model": model,
        "max_tokens": 4096,
        "output_config": output_config,
        "messages": [{ "role": "user", "content": prompt }]
    })
}

pub fn build_batch_body(model: &str, effort: Option<&str>, reqs: &[DescribeRequest]) -> Value {
    let requests: Vec<Value> = reqs
        .iter()
        .map(|r| {
            json!({
                "custom_id": r.custom_id,
                "params": build_message_body(model, effort, &build_prompt(r))
            })
        })
        .collect();
    json!({ "requests": requests })
}

pub fn parse_model_json(custom_id: &str, text: &str) -> Result<DescribeResponse> {
    let v: Value = serde_json::from_str(text)
        .map_err(|e| anyhow!("model output for {custom_id} was not valid JSON: {e}"))?;

    let what = v["what"]
        .as_str()
        .ok_or_else(|| anyhow!("missing `what` for {custom_id}"))?
        .to_string();

    let entry_symbol = v["entry_symbol"].as_str().map(|s| s.to_string());
    let empty: Vec<Value> = Vec::new();

    let key_symbols = v["key_symbols"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .filter_map(|item| {
            Some((
                item["name"].as_str()?.to_string(),
                item["description"].as_str()?.to_string(),
            ))
        })
        .collect();

    let look_here_for = v["look_here_for"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .filter_map(|s| s.as_str().map(|s| s.to_string()))
        .collect();

    let subdir_notes = v["subdir_notes"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .filter_map(|item| {
            Some((
                item["name"].as_str()?.to_string(),
                item["note"].as_str()?.to_string(),
            ))
        })
        .collect();

    Ok(DescribeResponse {
        custom_id: custom_id.to_string(),
        what,
        entry_symbol,
        key_symbols,
        look_here_for,
        subdir_notes,
    })
}

impl HttpLlm {
    fn client(&self) -> Result<reqwest::blocking::Client> {
        Ok(reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .build()?)
    }

    fn headers(&self) -> reqwest::header::HeaderMap {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert("x-api-key", self.api_key.parse().unwrap());
        h.insert("anthropic-version", ANTHROPIC_VERSION.parse().unwrap());
        h.insert("content-type", "application/json".parse().unwrap());
        h
    }

    fn describe_one_by_one(&self, reqs: Vec<DescribeRequest>) -> Result<Vec<DescribeResponse>> {
        let client = self.client()?;
        let mut out = Vec::new();

        for req in reqs {
            let body =
                build_message_body(&self.model, self.effort.as_deref(), &build_prompt(&req));
            let resp: Value = client
                .post(format!("{API_BASE}/v1/messages"))
                .headers(self.headers())
                .json(&body)
                .send()?
                .error_for_status()?
                .json()?;

            // With output_config.format set, the first text block is the JSON payload.
            let text = resp["content"]
                .as_array()
                .and_then(|blocks| blocks.iter().find(|b| b["type"] == "text"))
                .and_then(|b| b["text"].as_str())
                .ok_or_else(|| anyhow!("no text block in response for {}", req.custom_id))?;

            // Fail closed: a directory whose output we cannot parse is skipped,
            // leaving its previous map in place, rather than emitting a guess.
            match parse_model_json(&req.custom_id, text) {
                Ok(parsed) => out.push(parsed),
                Err(e) => eprintln!("warn: dropping {}: {e:#}", req.custom_id),
            }
        }
        Ok(out)
    }

    fn describe_batched(&self, reqs: Vec<DescribeRequest>) -> Result<Vec<DescribeResponse>> {
        let client = self.client()?;
        let body = build_batch_body(&self.model, self.effort.as_deref(), &reqs);

        let created: Value = client
            .post(format!("{API_BASE}/v1/messages/batches"))
            .headers(self.headers())
            .json(&body)
            .send()?
            .error_for_status()?
            .json()?;

        let batch_id = created["id"]
            .as_str()
            .ok_or_else(|| anyhow!("batch create returned no id"))?
            .to_string();
        eprintln!("batch {batch_id} submitted ({} requests)", reqs.len());

        loop {
            let status: Value = client
                .get(format!("{API_BASE}/v1/messages/batches/{batch_id}"))
                .headers(self.headers())
                .send()?
                .error_for_status()?
                .json()?;

            if status["processing_status"] == "ended" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(30));
        }

        let results_text = client
            .get(format!("{API_BASE}/v1/messages/batches/{batch_id}/results"))
            .headers(self.headers())
            .send()?
            .error_for_status()?
            .text()?;

        let mut out = Vec::new();
        // Results are JSONL and arrive in arbitrary order — key by custom_id.
        for line in results_text.lines().filter(|l| !l.trim().is_empty()) {
            let v: Value = serde_json::from_str(line)?;
            let custom_id = v["custom_id"].as_str().unwrap_or("").to_string();

            if v["result"]["type"] != "succeeded" {
                eprintln!("warn: batch entry {custom_id} did not succeed, skipping");
                continue;
            }
            let Some(text) = v["result"]["message"]["content"]
                .as_array()
                .and_then(|blocks| blocks.iter().find(|b| b["type"] == "text"))
                .and_then(|b| b["text"].as_str())
            else {
                eprintln!("warn: batch entry {custom_id} had no text block, skipping");
                continue;
            };
            match parse_model_json(&custom_id, text) {
                Ok(parsed) => out.push(parsed),
                Err(e) => eprintln!("warn: dropping {custom_id}: {e:#}"),
            }
        }
        Ok(out)
    }
}

impl Llm for HttpLlm {
    fn describe(&self, reqs: Vec<DescribeRequest>) -> Result<Vec<DescribeResponse>> {
        if reqs.is_empty() {
            return Ok(Vec::new());
        }
        if self.use_batch {
            self.describe_batched(reqs)
        } else {
            self.describe_one_by_one(reqs)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> DescribeRequest {
        DescribeRequest {
            custom_id: "d1".into(),
            dir_path: "src/settlement".into(),
            symbol_names: vec!["BatchBuilder".into(), "submit_batch".into()],
            file_names: vec!["batcher.rs".into(), "builder.rs".into()],
            child_summaries: vec![("adapters".into(), "Per-chain RPC quirks.".into())],
        }
    }

    #[test]
    fn prompt_lists_symbols_files_and_children() {
        let p = build_prompt(&req());
        assert!(p.contains("src/settlement"), "{p}");
        assert!(p.contains("BatchBuilder"), "{p}");
        assert!(p.contains("batcher.rs"), "{p}");
        assert!(p.contains("adapters"), "{p}");
        assert!(
            p.contains("exactly") || p.contains("copy"),
            "prompt must instruct the model to copy names verbatim: {p}"
        );
    }

    #[test]
    fn haiku_body_has_no_effort_field() {
        let body = build_message_body("claude-haiku-4-5", None, "hi");
        assert_eq!(body["model"], json!("claude-haiku-4-5"));
        assert_eq!(body["max_tokens"], json!(4096));
        assert!(
            body["output_config"].get("effort").is_none(),
            "effort errors on haiku-4-5: {body}"
        );
        assert_eq!(body["output_config"]["format"]["type"], json!("json_schema"));
    }

    #[test]
    fn opus_body_carries_effort() {
        let body = build_message_body("claude-opus-4-8", Some("low"), "hi");
        assert_eq!(body["output_config"]["effort"], json!("low"));
    }

    #[test]
    fn body_has_no_cache_control() {
        // Prompts are far below the 4096-token minimum cacheable prefix, so a
        // breakpoint would cost a write premium and never produce a read.
        let text = build_message_body("claude-haiku-4-5", None, "hi").to_string();
        assert!(!text.contains("cache_control"), "{text}");
    }

    #[test]
    fn batch_body_uses_custom_ids() {
        let body = build_batch_body("claude-haiku-4-5", None, &[req()]);
        assert_eq!(body["requests"][0]["custom_id"], json!("d1"));
        assert_eq!(
            body["requests"][0]["params"]["model"],
            json!("claude-haiku-4-5")
        );
    }

    #[test]
    fn parses_valid_model_json() {
        let text = r#"{"what":"Batching.","entry_symbol":"submit_batch","key_symbols":[{"name":"BatchBuilder","description":"builds"}],"look_here_for":["batching"],"subdir_notes":[{"name":"adapters","note":"rpc quirks"}]}"#;
        let r = parse_model_json("d1", text).unwrap();
        assert_eq!(r.custom_id, "d1");
        assert_eq!(r.what, "Batching.");
        assert_eq!(r.entry_symbol.as_deref(), Some("submit_batch"));
        assert_eq!(
            r.key_symbols,
            vec![("BatchBuilder".to_string(), "builds".to_string())]
        );
        assert_eq!(
            r.subdir_notes,
            vec![("adapters".to_string(), "rpc quirks".to_string())]
        );
    }

    #[test]
    fn parses_null_entry_symbol() {
        let text = r#"{"what":"x","entry_symbol":null,"key_symbols":[],"look_here_for":[],"subdir_notes":[]}"#;
        assert!(parse_model_json("d1", text).unwrap().entry_symbol.is_none());
    }

    #[test]
    fn unparseable_model_output_is_an_error_not_a_default() {
        assert!(parse_model_json("d1", "I'm sorry, I can't do that.").is_err());
    }
}
