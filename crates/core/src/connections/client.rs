//! 固定端点的连接验证和模型发现；不调用生成接口，不返回账户响应正文。
use super::{Model, Service, credentials::Credentials};
use galfriends_i18n::Message;
use reqwest::{
    blocking::{Client, RequestBuilder},
    header::{AUTHORIZATION, HeaderValue},
};
use serde_json::Value;
use std::io::Read;

/// 复用应用代理配置，禁止认证请求通过重定向转发。
pub fn client(proxy: &str) -> Result<Client, Message> {
    crate::sources::client_builder(proxy)
        .map_err(|_| Message::new("connection.proxy_error"))?
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| Message::new("connection.network_error"))
}
/// 拒绝空白、控制字符和过长密钥，避免头部注入。
pub fn validate_token(token: &str) -> Result<(), Message> {
    if token.is_empty() || token.len() > 1024 || !token.bytes().all(|b| b.is_ascii_graphic()) {
        Err(Message::new("ai.invalid_token"))
    } else {
        Ok(())
    }
}
/// 构造标记为敏感信息的认证头，VNDB 使用 Token 方案。
pub fn authorization(service: Service, token: &str) -> Result<HeaderValue, Message> {
    validate_token(token)?;
    let scheme = if service == Service::Vndb {
        "Token"
    } else {
        "Bearer"
    };
    let mut value = HeaderValue::from_str(&format!("{scheme} {token}"))
        .map_err(|_| Message::new("ai.invalid_token"))?;
    value.set_sensitive(true);
    Ok(value)
}
pub(super) fn request_json(request: RequestBuilder) -> Result<Value, Message> {
    let response = request
        .send()
        .map_err(|_| Message::new("connection.network_error"))?;
    if !response.status().is_success() {
        return Err(match response.status().as_u16() {
            401 | 403 => Message::new("connection.unauthorized"),
            429 => Message::new("connection.rate_limited"),
            code => Message::new("connection.http_error").arg("status", code),
        });
    }
    let mut bytes = Vec::new();
    response
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Message::new("connection.network_error"))?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(Message::new("connection.invalid_response"));
    }
    serde_json::from_slice(&bytes).map_err(|_| Message::new("connection.invalid_response"))
}
/// 返回服务的固定模型列表地址，数据源服务不支持该操作。
pub fn models_url(service: Service) -> Result<&'static str, Message> {
    match service {
        Service::Openai => Ok("https://api.openai.com/v1/models"),
        Service::Deepseek => Ok("https://api.deepseek.com/models"),
        Service::Qwen => Ok("https://dashscope.aliyuncs.com/compatible-mode/v1/models"),
        Service::Kimi => Ok("https://api.moonshot.cn/v1/models"),
        _ => Err(Message::new("ai.invalid_provider")),
    }
}
/// 获取当前密钥实际可用的模型，不以静态示例充当接口结果。
pub fn models(client: &Client, service: Service, token: &str) -> Result<Vec<Model>, Message> {
    let value = request_json(
        client
            .get(models_url(service)?)
            .header(AUTHORIZATION, authorization(service, token)?),
    )?;
    parse_models(&value)
}
fn parse_models(value: &Value) -> Result<Vec<Model>, Message> {
    let rows = value["data"]
        .as_array()
        .ok_or_else(|| Message::new("connection.invalid_response"))?;
    let mut result = Vec::new();
    for row in rows {
        let id = row["id"]
            .as_str()
            .filter(|s| !s.trim().is_empty() && s.len() <= 256)
            .ok_or_else(|| Message::new("connection.invalid_response"))?;
        result.push(Model {
            id: id.into(),
            name: row["name"].as_str().unwrap_or(id).into(),
        });
    }
    result.sort_by(|a, b| a.id.cmp(&b.id));
    result.dedup_by(|a, b| a.id == b.id);
    Ok(result)
}
/// 检验数据源凭据。公开模式不需要保存空令牌。
pub fn check_source(
    client: &Client,
    service: Service,
    credentials: &Credentials,
) -> Result<(), Message> {
    let url = match service {
        Service::Bangumi => "https://api.bgm.tv/v0/me",
        Service::Vndb => "https://api.vndb.org/kana/authinfo",
        Service::Kungal => "https://api.nextmoe.dev/v2/catalog/characters?limit=1",
        _ => return Err(Message::new("connection.invalid_service")),
    };
    let value = request_json(
        client
            .get(url)
            .header(AUTHORIZATION, authorization(service, &credentials.token)?),
    )?;
    let valid = if service == Service::Kungal {
        value["items"].is_array()
    } else {
        !value["id"].is_null()
    };
    if valid {
        Ok(())
    } else {
        Err(Message::new("connection.invalid_response"))
    }
}
/// 只允许固定帮助页面，不接受前端传入的任意 URL。
pub fn website(service: &str) -> Result<&'static str, Message> {
    match service {
        "bangumi" => Ok("https://next.bgm.tv/demo/access-token"),
        "kungal" => Ok("https://developer.nextmoe.dev/"),
        "vndb" => Ok("https://vndb.org/u/tokens"),
        "erogamescape" => Ok("https://erogamescape.dyndns.org/~ap2/ero/toukei_kaiseki/"),
        _ => Err(Message::new("connection.invalid_service")),
    }
}
#[cfg(test)]
mod tests {
    use super::{Service, authorization, parse_models, request_json};
    #[test]
    fn malformed_models_and_header_injection_are_rejected() {
        assert!(parse_models(&serde_json::json!({"data":[{"unexpected":"field"}]})).is_err());
        assert!(authorization(Service::Bangumi, "key\r\nInjected: yes").is_err());
        assert_eq!(
            authorization(Service::Vndb, "fixture").unwrap(),
            "Token fixture"
        );
        let models =
            parse_models(&serde_json::json!({"data":[{"id":"z"},{"id":"a"},{"id":"z"}]})).unwrap();
        assert_eq!(
            models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["a", "z"]
        );
    }
    #[test]
    fn http_failures_do_not_expose_response_body_or_retry() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0; 4096];
            assert!(stream.read(&mut buf).unwrap() > 0);
            write!(stream,"HTTP/1.1 401 Unauthorized\r\nContent-Length: 14\r\nConnection: close\r\n\r\nprivate-secret").unwrap();
        });
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(3))
            .build()
            .unwrap();
        let error = request_json(client.get(url)).unwrap_err();
        worker.join().unwrap();
        assert_eq!(error.key, "connection.unauthorized");
        assert!(!error.to_string().contains("private-secret"));
    }
}
