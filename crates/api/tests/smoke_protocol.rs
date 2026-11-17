use ai_phone_api::wire::{ApiError, RegistryRequest, RegistryState};
use prost::Message;
use reqwest::{Client, StatusCode, Url, header::CONTENT_TYPE};
use std::{env, fs, time::Duration};

const PROTOBUF: &str = "application/x-protobuf";

#[tokio::test]
#[ignore = "requires a running Assistant API and its token file"]
async fn registry_protobuf_http() -> Result<(), Box<dyn std::error::Error>> {
    let base = env::var("AI_PHONE_SMOKE_API")
        .ok()
        .filter(|base| !base.is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:17863".into());
    let endpoint = Url::parse(&base)?.join("/v1/registry/state")?;
    let token_file = env::var_os("AI_PHONE_SMOKE_TOKEN_FILE")
        .filter(|path| !path.is_empty())
        .unwrap_or_else(|| "data/assistant/token".into());
    let project = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let token = fs::read_to_string(project.join(token_file))?;
    let client = Client::builder().timeout(Duration::from_secs(10)).build()?;
    let post = |body, media, authenticated| {
        let request = client
            .post(endpoint.clone())
            .header(CONTENT_TYPE, media)
            .body(body);
        if authenticated {
            request.bearer_auth(token.trim())
        } else {
            request
        }
    };
    let body = RegistryRequest {}.encode_to_vec();
    let response = post(body.clone(), PROTOBUF, true).send().await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(CONTENT_TYPE)
            .map(|value| value.as_bytes()),
        Some(PROTOBUF.as_bytes())
    );
    let registry = RegistryState::decode(response.bytes().await?)?;
    assert!(
        registry
            .devices
            .iter()
            .any(|device| device.id == registry.active_device_id)
    );

    for (body, media, authenticated, status, code) in [
        (
            vec![255],
            PROTOBUF,
            true,
            StatusCode::BAD_REQUEST,
            "invalid_protobuf",
        ),
        (
            body.clone(),
            "application/json",
            true,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
        ),
        (
            body,
            PROTOBUF,
            false,
            StatusCode::UNAUTHORIZED,
            "unauthorized",
        ),
    ] {
        let response = post(body, media, authenticated).send().await?;
        assert_eq!(response.status(), status);
        let error = ApiError::decode(response.bytes().await?)?;
        assert_eq!(error.code, code);
    }
    Ok(())
}
