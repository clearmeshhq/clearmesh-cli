use crate::config::GlobalConfig;
use anyhow::{anyhow, Result};
use reqwest::header::{HeaderName, HeaderValue};
use reqwest::{Client, Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize)]
pub struct PresignedChunkUrl {
    pub method: String,
    pub url: String,
    pub headers: BTreeMap<String, String>,
}

#[derive(Clone)]
pub struct ApiClient {
    base: String,
    token: Option<String>,
    client: Client,
}

impl ApiClient {
    pub fn from_config(config: &GlobalConfig) -> Self {
        Self {
            base: config.api.trim_end_matches('/').to_string(),
            token: config.token.clone(),
            client: Client::new(),
        }
    }

    pub async fn get(&self, path: &str) -> Result<Value> {
        self.request(Method::GET, path, Option::<&()>::None).await
    }

    pub async fn post<T: Serialize + ?Sized>(&self, path: &str, body: &T) -> Result<Value> {
        self.request(Method::POST, path, Some(body)).await
    }

    pub async fn patch<T: Serialize + ?Sized>(&self, path: &str, body: &T) -> Result<Value> {
        self.request(Method::PATCH, path, Some(body)).await
    }

    pub async fn delete(&self, path: &str) -> Result<Value> {
        self.request(Method::DELETE, path, Option::<&()>::None)
            .await
    }

    pub async fn put_presigned(&self, signed: &PresignedChunkUrl, bytes: Vec<u8>) -> Result<()> {
        if signed.method != "PUT" {
            return Err(anyhow!("presigned URL method mismatch"));
        }
        let response = self
            .apply_presigned_headers(self.client.put(&signed.url), &signed.headers)?
            .body(bytes)
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!("direct Vault upload failed with {status}"));
        }
        Ok(())
    }

    pub async fn get_presigned(&self, signed: &PresignedChunkUrl) -> Result<Vec<u8>> {
        if signed.method != "GET" {
            return Err(anyhow!("presigned URL method mismatch"));
        }
        let response = self
            .apply_presigned_headers(self.client.get(&signed.url), &signed.headers)?
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!("direct Vault download failed with {status}"));
        }
        Ok(response.bytes().await?.to_vec())
    }

    async fn request<T: Serialize + ?Sized>(
        &self,
        method: Method,
        path: &str,
        body: Option<&T>,
    ) -> Result<Value> {
        let mut request = self.client.request(method, self.url(path));
        request = self.authorized(request);
        if let Some(body) = body {
            request = request.json(body);
        }
        parse_response(request.send().await?).await
    }

    fn authorized(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(token) = &self.token {
            request.bearer_auth(token)
        } else {
            request
        }
    }

    fn apply_presigned_headers(
        &self,
        mut request: reqwest::RequestBuilder,
        headers: &BTreeMap<String, String>,
    ) -> Result<reqwest::RequestBuilder> {
        for (name, value) in headers {
            request = request.header(
                HeaderName::from_bytes(name.as_bytes())?,
                HeaderValue::from_str(value)?,
            );
        }
        Ok(request)
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }
}

async fn parse_response(response: reqwest::Response) -> Result<Value> {
    let status = response.status();
    if status == StatusCode::NO_CONTENT {
        return Ok(serde_json::json!({}));
    }
    let text = response.text().await?;
    if !status.is_success() {
        return Err(anyhow!("API request failed with {status}: {text}"));
    }
    Ok(serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({ "raw": text })))
}
