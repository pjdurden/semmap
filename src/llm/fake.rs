use super::{DescribeRequest, DescribeResponse, Llm};
use anyhow::Result;
use std::collections::HashMap;

#[derive(Debug, Default, Clone)]
pub struct FakeLlm {
    pub responses: HashMap<String, DescribeResponse>,
}

impl Llm for FakeLlm {
    fn describe(&self, reqs: Vec<DescribeRequest>) -> Result<Vec<DescribeResponse>> {
        Ok(reqs
            .iter()
            .filter_map(|r| self.responses.get(&r.custom_id).cloned())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_canned_response_by_custom_id_and_skips_unknown() {
        let mut responses = HashMap::new();
        responses.insert(
            "d1".to_string(),
            DescribeResponse {
                custom_id: "d1".into(),
                what: "hello".into(),
                ..Default::default()
            },
        );
        let fake = FakeLlm { responses };

        let out = fake
            .describe(vec![
                DescribeRequest {
                    custom_id: "d1".into(),
                    dir_path: "src".into(),
                    symbol_names: vec![],
                    file_names: vec![],
                    child_summaries: vec![],
                },
                DescribeRequest {
                    custom_id: "unknown".into(),
                    dir_path: "other".into(),
                    symbol_names: vec![],
                    file_names: vec![],
                    child_summaries: vec![],
                },
            ])
            .unwrap();

        assert_eq!(out.len(), 1, "requests with no canned response are dropped");
        assert_eq!(out[0].what, "hello");
    }
}
