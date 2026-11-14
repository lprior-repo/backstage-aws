use super::*;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    thread,
};

struct Server {
    address: String,
    worker: thread::JoinHandle<Vec<String>>,
}

impl Server {
    fn start(responses: Vec<(u16, String)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local server");
        let address = listener.local_addr().expect("local address").to_string();
        let worker = thread::spawn(move || {
            responses
                .into_iter()
                .map(|(status, body)| {
                    let (stream, _) = listener.accept().expect("accept request");
                    respond(stream, status, &body)
                })
                .collect()
        });
        Self { address, worker }
    }

    fn finish(self) -> Vec<String> {
        self.worker.join().expect("server thread")
    }
}

fn respond(stream: TcpStream, status: u16, body: &str) -> String {
    let mut request = String::new();
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let mut content_length = 0;
    loop {
        line.clear();
        reader.read_line(&mut line).expect("read request header");
        if line == "\r\n" || line.is_empty() {
            break;
        }
        if let Some(value) = line
            .strip_prefix("content-length:")
            .or_else(|| line.strip_prefix("Content-Length:"))
        {
            content_length = value.trim().parse::<usize>().expect("content length");
        }
        request.push_str(&line);
    }
    let mut payload = vec![0; content_length];
    reader.read_exact(&mut payload).expect("read request body");
    request.push_str(&String::from_utf8(payload).expect("utf8 request body"));
    let mut stream = reader.into_inner();
    write!(
        stream,
        "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .expect("write response");
    request
}

fn client(address: &str) -> Octocrab {
    Octocrab::builder()
        .base_uri(format!("http://{address}"))
        .expect("base uri")
        .personal_token("test-token")
        .build()
        .expect("build client")
}

fn user_response() -> String {
    json!({"login": "trusted-bot"}).to_string()
}

fn comment_response(id: u64) -> String {
    json!({"id": id, "html_url": format!("https://github.test/comments/{id}")}).to_string()
}

#[tokio::test]
async fn creates_comment_when_no_owned_marker_exists() {
    let server = Server::start(vec![
        (200, user_response()),
        (200, "[]".to_owned()),
        (201, comment_response(12)),
    ]);
    let result = comment_with_client(
        &client(&server.address),
        CommentIdentity::AuthenticatedLogin,
        "acme/site",
        42,
        "preview",
        "deployment ready",
    )
    .await;
    assert_eq!(
        result.expect("comment request"),
        Some("https://github.test/comments/12".to_owned())
    );
    let requests = server.finish();
    assert!(requests[0].starts_with("GET /user "));
    assert!(
        requests[1].starts_with("GET /repos/acme/site/issues/42/comments?per_page=100&page=1 ")
    );
    assert!(requests[2].starts_with("POST /repos/acme/site/issues/42/comments "));
    assert!(requests[2].contains("<!-- backstage-aws:preview -->\\ndeployment ready"));
}

#[tokio::test]
async fn patches_only_authenticated_owners_comment_after_first_page() {
    let marker = "<!-- backstage-aws:preview -->";
    let mut first_page = (0..COMMENT_PAGE_SIZE - 1)
        .map(|id| json!({"id": id + 1, "body": "other", "user": {"login": "trusted-bot"}}))
        .collect::<Vec<_>>();
    first_page.push(json!({
        "id": 500,
        "body": format!("{marker}\nspoofed"),
        "user": {"login": "someone-else"}
    }));
    let second_page = json!([{
        "id": 900,
        "body": format!("{marker}\nold"),
        "user": {"login": "TRUSTED-BOT"}
    }]);
    let server = Server::start(vec![
        (200, user_response()),
        (200, Value::Array(first_page).to_string()),
        (200, second_page.to_string()),
        (200, comment_response(900)),
    ]);
    let result = comment_with_client(
        &client(&server.address),
        CommentIdentity::AuthenticatedLogin,
        "acme/site",
        42,
        "preview",
        "updated",
    )
    .await;
    assert_eq!(
        result.expect("comment request"),
        Some("https://github.test/comments/900".to_owned())
    );
    let requests = server.finish();
    assert!(
        requests[1].starts_with("GET /repos/acme/site/issues/42/comments?per_page=100&page=1 ")
    );
    assert!(
        requests[2].starts_with("GET /repos/acme/site/issues/42/comments?per_page=100&page=2 ")
    );
    assert!(requests[3].starts_with("PATCH /repos/acme/site/issues/comments/900 "));
    assert!(requests[3].contains("updated"));
    assert!(
        !requests
            .iter()
            .any(|request| request.contains("/comments/500"))
    );
}

#[tokio::test]
async fn upserts_actions_bot_comment_without_user_lookup() {
    let marker = "<!-- backstage-aws:preview -->";
    let comments = json!([
        {
            "id": 500,
            "body": format!("{marker}\nspoofed"),
            "user": {"id": 7, "login": "spoof"}
        },
        {
            "id": 900,
            "body": format!("{marker}\nold"),
            "user": {"id": ACTIONS_BOT_ID, "login": "github-actions[bot]"}
        }
    ]);
    let server = Server::start(vec![
        (200, comments.to_string()),
        (200, comment_response(900)),
    ]);
    let result = comment_with_client(
        &client(&server.address),
        CommentIdentity::ActionsBot,
        "acme/site",
        42,
        "preview",
        "updated",
    )
    .await;
    assert_eq!(
        result.expect("comment request"),
        Some("https://github.test/comments/900".to_owned())
    );
    let requests = server.finish();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("GET /repos/acme/site/issues/42/comments?"));
    assert!(requests[1].starts_with("PATCH /repos/acme/site/issues/comments/900 "));
    assert!(
        !requests
            .iter()
            .any(|request| request.starts_with("GET /user "))
    );
}

#[tokio::test]
async fn returns_github_api_errors() {
    let server = Server::start(vec![(403, json!({"message": "forbidden"}).to_string())]);
    let result = comment_with_client(
        &client(&server.address),
        CommentIdentity::AuthenticatedLogin,
        "acme/site",
        42,
        "preview",
        "deployment ready",
    )
    .await;
    assert!(result.is_err());
    assert!(server.finish()[0].starts_with("GET /user "));
}

#[tokio::test]
async fn stops_when_comment_lookup_api_fails() {
    let server = Server::start(vec![
        (200, user_response()),
        (403, json!({"message": "forbidden"}).to_string()),
    ]);
    let result = comment_with_client(
        &client(&server.address),
        CommentIdentity::AuthenticatedLogin,
        "acme/site",
        42,
        "preview",
        "deployment ready",
    )
    .await;
    assert!(result.is_err());
    let requests = server.finish();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].starts_with("GET /repos/acme/site/issues/42/comments?"));
}

#[tokio::test]
async fn refuses_to_create_after_bounded_full_pagination() {
    let mut responses = vec![(200, user_response())];
    responses.extend((0..COMMENT_PAGE_LIMIT).map(|_| {
        let page = (0..COMMENT_PAGE_SIZE)
            .map(|id| json!({"id": id, "body": "other", "user": {"login": "trusted-bot"}}))
            .collect::<Vec<_>>();
        (200, Value::Array(page).to_string())
    }));
    let server = Server::start(responses);
    let result = comment_with_client(
        &client(&server.address),
        CommentIdentity::AuthenticatedLogin,
        "acme/site",
        42,
        "preview",
        "deployment ready",
    )
    .await;
    assert!(result.is_err());
    let requests = server.finish();
    assert_eq!(requests.len(), COMMENT_PAGE_LIMIT + 1);
    assert!(requests.last().is_some_and(|request| {
        request.starts_with(&format!(
            "GET /repos/acme/site/issues/42/comments?per_page=100&page={COMMENT_PAGE_LIMIT} "
        ))
    }));
}
