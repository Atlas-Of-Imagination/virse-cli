use axum::{Router, http::StatusCode, routing::post};
use virse_api::CliAuthClient;

#[tokio::test]
async fn gateway_non_json_errors_preserve_status_without_echoing_body() {
    for (status, body, expected) in [
        (503, "no healthy upstream", "AUTH_SERVICE_UNAVAILABLE"),
        (
            502,
            "<html>bad gateway with sensitive data</html>",
            "AUTH_SERVICE_UNAVAILABLE",
        ),
        (429, "rate limited", "AUTH_RATE_LIMITED"),
        (403, "<html>denied</html>", "AUTHORIZATION_DENIED"),
        (302, "redirect", "AUTHORIZATION_REDIRECT_REJECTED"),
        (
            200,
            "<html>unexpected successful page</html>",
            "INVALID_RESPONSE",
        ),
        (
            503,
            r#"{"detail":{"code":"CLI_AUTH_NOT_CONFIGURED"}}"#,
            "CLI_AUTH_NOT_CONFIGURED",
        ),
    ] {
        let app = Router::new().route(
            "/auth/cli/start",
            post(move || async move {
                (
                    StatusCode::from_u16(status).unwrap(),
                    [("x-request-id", "request-123")],
                    body,
                )
            }),
        );
        let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client =
            CliAuthClient::new(&format!("http://{}", socket.local_addr().unwrap())).unwrap();
        let server = tokio::spawn(async move { axum::serve(socket, app).await.unwrap() });
        let error = match client
            .start("device", &"a".repeat(43), &"s".repeat(32), None, "test")
            .await
        {
            Err(error) => error,
            Ok(_) => panic!("Unexpected success"),
        };
        assert_eq!(error.status, Some(status));
        assert_eq!(error.code, expected);
        assert_eq!(error.request_id.as_deref(), Some("request-123"));
        assert!(error.message.contains(&format!("HTTP {status}")));
        assert!(!error.message.contains(body));
        assert_eq!(error.retryable, status == 429 || status >= 500);
        server.abort();
    }
}
