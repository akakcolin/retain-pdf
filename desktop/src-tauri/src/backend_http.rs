use std::time::Duration;

use serde_json::Value;

pub fn can_reuse_existing_backend(api_port: u16, api_key: &str) -> bool {
    let health_url = format!("http://127.0.0.1:{api_port}/health");
    let health = match request_json(&health_url, None, 2000) {
        Ok(payload) => payload,
        Err(_) => return false,
    };
    let healthy = health
        .pointer("/data/status")
        .and_then(Value::as_str)
        .map(|status| status == "up")
        .unwrap_or(false);
    if !healthy {
        return false;
    }
    let jobs_url = format!("http://127.0.0.1:{api_port}/api/v1/jobs?limit=1&offset=0");
    match request_json(&jobs_url, Some(("x-api-key", api_key)), 3000) {
        Ok(payload) => payload.pointer("/data/items").map(Value::is_array).unwrap_or(false),
        Err(error) => {
            eprintln!(
                "[desktop] existing backend on :{api_port} is not reusable: {error}"
            );
            false
        }
    }
}

fn request_json(url: &str, header: Option<(&str, &str)>, timeout_ms: u64) -> Result<Value, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_millis(timeout_ms))
        .build();
    let mut request = agent.get(url);
    if let Some((key, value)) = header {
        request = request.set(key, value);
    }
    let response = request.call().map_err(|error| error.to_string())?;
    response.into_json::<Value>().map_err(|error| error.to_string())
}
