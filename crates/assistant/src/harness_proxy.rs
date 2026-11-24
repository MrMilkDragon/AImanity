use crate::{Shared, decode, error, harness_proxy_devices, pb, wire};
pub(crate) use aimanity_workbench_proxy::{ADDRESS, HarnessProxy, download_client};
use axum::{
    body::Bytes,
    extract::{Path, State, rejection::BytesRejection},
    http::{HeaderMap, StatusCode},
    response::Response,
};

pub async fn resume(app: Shared) {
    let proxy = &app.settings.harness_proxy;
    if matches!(proxy.restore().await, Ok(true)) {
        if let Err(error) = crate::device_setup::wait_for_runtime(&app.root).await {
            eprintln!("Workbench proxy device readiness unavailable: {error}");
            return;
        }
        if let Err(error) = proxy
            .apply_enabled(harness_proxy_devices::apply(&app, true))
            .await
        {
            eprintln!("Workbench proxy devices were not restored: {error}");
        }
    }
}

pub async fn route(
    State(app): State<Shared>,
    Path(operation): Path<String>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let input = match decode::<wire::HarnessProxyRequest>(&app, &headers, body) {
        Ok(input) => input,
        Err(error) => return error.into(),
    };
    if !matches!(
        operation.as_str(),
        "state" | "import" | "refresh" | "toggle" | "select"
    ) {
        return error(StatusCode::NOT_FOUND, "not_found", "未知代理操作");
    }
    match app
        .settings
        .harness_proxy
        .request(&operation, input, |enabled| {
            harness_proxy_devices::apply(&app, enabled)
        })
        .await
    {
        Ok(state) => pb(StatusCode::OK, state),
        Err(_) => error(
            StatusCode::BAD_REQUEST,
            "harness_proxy_error",
            "工作台代理操作失败",
        ),
    }
}
