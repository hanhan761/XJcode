use super::*;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
async fn fixture(body: String, status: &str) -> (ApiProfile, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let status = status.to_owned();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 65536];
        let n = socket.read(&mut buf).await.unwrap();
        let request = String::from_utf8_lossy(&buf[..n]).to_string();
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(response.as_bytes()).await.unwrap();
        request
    });
    (
        ApiProfile {
            base_url: format!("http://{address}/v1"),
            model: "fixture".into(),
            wire_api: "chat".into(),
            token: "mock-test-token".into(),
            supports_images: false,
            context_window: 32768,
        },
        server,
    )
}
#[tokio::test]
async fn chat_stream_is_complete_before_reply_is_returned() {
    let (p,server)=fixture("data: {\"choices\":[{\"delta\":{\"content\":\"hello\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n".into(),"200 OK").await;
    assert_eq!(chat(&p, "test instructions", &[]).await.unwrap(), "hello");
    let req = server.await.unwrap();
    assert!(req.starts_with("POST /v1/chat/completions"));
    assert!(
        req.to_lowercase()
            .contains("authorization: bearer mock-test-token")
    );
}
#[tokio::test]
async fn interrupted_error_and_truncated_streams_never_return_partial_tools() {
    for (body, status) in [
        (
            "data: {\"choices\":[{\"delta\":{\"content\":\"partial tool data\"}}]}\n\n",
            "200 OK",
        ),
        (
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\ndata: [DONE]\n\n",
            "200 OK",
        ),
        ("{\"error\":\"private server data\"}", "401 Unauthorized"),
    ] {
        let (p, server) = fixture(body.into(), status).await;
        let error = chat(&p, "task", &[]).await.unwrap_err().to_string();
        assert!(!error.contains("private server data"));
        assert!(!error.contains("mock-test-token"));
        server.await.unwrap();
    }
}
