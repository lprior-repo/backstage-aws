use super::*;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    thread,
    time::{Duration, Instant},
};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

struct Server {
    address: String,
    worker: thread::JoinHandle<std::io::Result<Vec<String>>>,
}

impl Server {
    fn start(responses: Vec<(u16, String)>) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?.to_string();
        let worker = thread::spawn(move || {
            responses
                .into_iter()
                .map(|(status, body)| {
                    let started = Instant::now();
                    let (stream, _) = loop {
                        match listener.accept() {
                            Ok(connection) => break connection,
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                if started.elapsed() >= Duration::from_secs(1) {
                                    return Err(std::io::Error::new(
                                        std::io::ErrorKind::TimedOut,
                                        "expected request was not received",
                                    ));
                                }
                                thread::sleep(Duration::from_millis(5));
                            }
                            Err(error) => return Err(error),
                        }
                    };
                    respond(stream, status, &body)
                })
                .collect()
        });
        Ok(Self { address, worker })
    }

    fn finish(self) -> std::result::Result<Vec<String>, Box<dyn std::error::Error>> {
        let requests = self.worker.join().map_err(|_| "server thread panicked")??;
        Ok(requests)
    }
}

fn request(requests: &[String], index: usize) -> std::result::Result<&str, String> {
    requests
        .get(index)
        .map(String::as_str)
        .ok_or_else(|| format!("missing recorded request {index}"))
}

fn respond(stream: TcpStream, status: u16, body: &str) -> std::io::Result<String> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut request = String::new();
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let mut content_length = 0;
    loop {
        line.clear();
        reader.read_line(&mut line)?;
        if line == "\r\n" || line.is_empty() {
            break;
        }
        if let Some(value) = line
            .strip_prefix("content-length:")
            .or_else(|| line.strip_prefix("Content-Length:"))
        {
            content_length = value
                .trim()
                .parse::<usize>()
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        }
        request.push_str(&line);
    }
    if content_length > 1_000_000 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "request body exceeds fixture limit",
        ));
    }
    let mut payload = vec![0; content_length];
    reader.read_exact(&mut payload)?;
    request.push_str(
        &String::from_utf8(payload)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?,
    );
    let mut stream = reader.into_inner();
    write!(
        stream,
        "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    Ok(request)
}

fn client(address: &str) -> octocrab::Result<Octocrab> {
    Octocrab::builder()
        .base_uri(format!("http://{address}"))?
        .personal_token("test-token")
        .build()
}

fn user_response() -> String {
    json!({"login": "trusted-bot"}).to_string()
}

fn comment_response(id: u64) -> String {
    json!({"id": id, "html_url": format!("https://github.test/comments/{id}")}).to_string()
}

#[test]
fn comment_identity_only_allows_installation_fallback_for_actions_github_token() {
    assert_eq!(
        comment_identity(Some("true"), true),
        CommentIdentity::ActionsInstallationToken
    );
    assert_eq!(
        comment_identity(Some("true"), false),
        CommentIdentity::AuthenticatedLogin
    );
    assert_eq!(
        comment_identity(Some("false"), true),
        CommentIdentity::AuthenticatedLogin
    );
    assert_eq!(
        comment_identity(None, true),
        CommentIdentity::AuthenticatedLogin
    );
}

#[tokio::test]
async fn installation_identity_fallback_does_not_hide_unrelated_user_lookup_errors() -> TestResult {
    let server = Server::start(vec![(
        500,
        json!({"message": "server failure"}).to_string(),
    )])?;
    let result = comment_with_client(
        &client(&server.address)?,
        CommentIdentity::ActionsInstallationToken,
        "acme/site",
        42,
        "preview",
        "deployment ready",
    )
    .await;
    assert!(result.is_err());
    let requests = server.finish()?;
    assert_eq!(requests.len(), 1);
    assert!(request(&requests, 0)?.starts_with("GET /user "));
    Ok(())
}

#[tokio::test]
async fn creates_comment_when_no_owned_marker_exists() -> TestResult {
    let server = Server::start(vec![
        (200, user_response()),
        (200, "[]".to_owned()),
        (201, comment_response(12)),
    ])?;
    let result = comment_with_client(
        &client(&server.address)?,
        CommentIdentity::AuthenticatedLogin,
        "acme/site",
        42,
        "preview",
        "deployment ready",
    )
    .await;
    assert_eq!(result?, Some("https://github.test/comments/12".to_owned()));
    let requests = server.finish()?;
    assert!(request(&requests, 0)?.starts_with("GET /user "));
    assert!(
        request(&requests, 1)?
            .starts_with("GET /repos/acme/site/issues/42/comments?per_page=100&page=1 ")
    );
    assert!(request(&requests, 2)?.starts_with("POST /repos/acme/site/issues/42/comments "));
    assert!(request(&requests, 2)?.contains("<!-- backstage-aws:preview -->\\ndeployment ready"));
    Ok(())
}

#[tokio::test]
async fn patches_only_authenticated_owners_comment_after_first_page() -> TestResult {
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
    ])?;
    let result = comment_with_client(
        &client(&server.address)?,
        CommentIdentity::AuthenticatedLogin,
        "acme/site",
        42,
        "preview",
        "updated",
    )
    .await;
    assert_eq!(result?, Some("https://github.test/comments/900".to_owned()));
    let requests = server.finish()?;
    assert!(
        request(&requests, 1)?
            .starts_with("GET /repos/acme/site/issues/42/comments?per_page=100&page=1 ")
    );
    assert!(
        request(&requests, 2)?
            .starts_with("GET /repos/acme/site/issues/42/comments?per_page=100&page=2 ")
    );
    assert!(request(&requests, 3)?.starts_with("PATCH /repos/acme/site/issues/comments/900 "));
    assert!(request(&requests, 3)?.contains("updated"));
    assert!(
        !requests
            .iter()
            .any(|request| request.contains("/comments/500"))
    );
    Ok(())
}

#[tokio::test]
async fn installation_token_falls_back_to_the_actions_bot_after_user_lookup_is_denied() -> TestResult
{
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
        (
            403,
            json!({"message": "installation token has no user"}).to_string(),
        ),
        (200, comments.to_string()),
        (200, comment_response(900)),
    ])?;
    let result = comment_with_client(
        &client(&server.address)?,
        CommentIdentity::ActionsInstallationToken,
        "acme/site",
        42,
        "preview",
        "updated",
    )
    .await;
    assert_eq!(result?, Some("https://github.test/comments/900".to_owned()));
    let requests = server.finish()?;
    assert_eq!(requests.len(), 3);
    assert!(request(&requests, 0)?.starts_with("GET /user "));
    assert!(request(&requests, 1)?.starts_with("GET /repos/acme/site/issues/42/comments?"));
    assert!(request(&requests, 2)?.starts_with("PATCH /repos/acme/site/issues/comments/900 "));
    assert!(
        !requests
            .iter()
            .any(|request| request.starts_with("POST /repos/acme/site/issues/42/comments "))
    );
    Ok(())
}

#[tokio::test]
async fn actions_pat_uses_the_authenticated_user_instead_of_the_actions_bot() -> TestResult {
    let marker = "<!-- backstage-aws:preview -->";
    let comments = json!([{
        "id": 901,
        "body": format!("{marker}\nold"),
        "user": {"id": 7, "login": "pat-owner"}
    }]);
    let server = Server::start(vec![
        (200, json!({"login": "pat-owner"}).to_string()),
        (200, comments.to_string()),
        (200, comment_response(901)),
    ])?;
    let result = comment_with_client(
        &client(&server.address)?,
        CommentIdentity::ActionsInstallationToken,
        "acme/site",
        42,
        "preview",
        "updated by PAT",
    )
    .await;
    assert_eq!(result?, Some("https://github.test/comments/901".to_owned()));
    let requests = server.finish()?;
    assert_eq!(requests.len(), 3);
    assert!(request(&requests, 0)?.starts_with("GET /user "));
    assert!(request(&requests, 1)?.starts_with("GET /repos/acme/site/issues/42/comments?"));
    assert!(request(&requests, 2)?.starts_with("PATCH /repos/acme/site/issues/comments/901 "));
    assert!(request(&requests, 2)?.contains("updated by PAT"));
    Ok(())
}

#[tokio::test]
async fn returns_github_api_errors() -> TestResult {
    let server = Server::start(vec![(403, json!({"message": "forbidden"}).to_string())])?;
    let result = comment_with_client(
        &client(&server.address)?,
        CommentIdentity::AuthenticatedLogin,
        "acme/site",
        42,
        "preview",
        "deployment ready",
    )
    .await;
    assert!(result.is_err());
    assert!(request(&server.finish()?, 0)?.starts_with("GET /user "));
    Ok(())
}

#[tokio::test]
async fn stops_when_comment_lookup_api_fails() -> TestResult {
    let server = Server::start(vec![
        (200, user_response()),
        (403, json!({"message": "forbidden"}).to_string()),
    ])?;
    let result = comment_with_client(
        &client(&server.address)?,
        CommentIdentity::AuthenticatedLogin,
        "acme/site",
        42,
        "preview",
        "deployment ready",
    )
    .await;
    assert!(result.is_err());
    let requests = server.finish()?;
    assert_eq!(requests.len(), 2);
    assert!(request(&requests, 1)?.starts_with("GET /repos/acme/site/issues/42/comments?"));
    Ok(())
}

#[tokio::test]
async fn refuses_to_create_after_bounded_full_pagination() -> TestResult {
    let mut responses = vec![(200, user_response())];
    responses.extend((0..COMMENT_PAGE_LIMIT).map(|_| {
        let page = (0..COMMENT_PAGE_SIZE)
            .map(|id| json!({"id": id, "body": "other", "user": {"login": "trusted-bot"}}))
            .collect::<Vec<_>>();
        (200, Value::Array(page).to_string())
    }));
    let server = Server::start(responses)?;
    let result = comment_with_client(
        &client(&server.address)?,
        CommentIdentity::AuthenticatedLogin,
        "acme/site",
        42,
        "preview",
        "deployment ready",
    )
    .await;
    assert!(result.is_err());
    let requests = server.finish()?;
    assert_eq!(requests.len(), COMMENT_PAGE_LIMIT + 1);
    assert!(requests.last().is_some_and(|request| {
        request.starts_with(&format!(
            "GET /repos/acme/site/issues/42/comments?per_page=100&page={COMMENT_PAGE_LIMIT} "
        ))
    }));
    Ok(())
}

#[tokio::test]
async fn publish_push_targets_the_approved_url_not_origin_pushurl() -> TestResult {
    use std::{ffi::OsStr, fs, process::Command};

    let temp = tempfile::tempdir()?;
    let target = temp.path().join("approved.git");
    let redirected = temp.path().join("redirected.git");
    let work = temp.path().join("work");
    let git = |args: &[&str], cwd: Option<&std::path::Path>| -> TestResult {
        let mut command = Command::new("git");
        command.args(args);
        if let Some(directory) = cwd {
            command.current_dir(directory);
        }
        let output = command.output()?;
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    };
    git(
        &[
            "init",
            "--bare",
            "--initial-branch=main",
            target.to_str().ok_or("fixture path is not UTF-8")?,
        ],
        None,
    )?;
    git(
        &[
            "init",
            "--bare",
            "--initial-branch=main",
            redirected.to_str().ok_or("fixture path is not UTF-8")?,
        ],
        None,
    )?;
    git(
        &[
            "init",
            "--initial-branch=main",
            work.to_str().ok_or("fixture path is not UTF-8")?,
        ],
        None,
    )?;
    git(&["config", "user.name", "Test Publisher"], Some(&work))?;
    git(
        &["config", "user.email", "publisher@example.test"],
        Some(&work),
    )?;
    fs::write(work.join("project.txt"), "publish target regression")?;
    git(&["add", "project.txt"], Some(&work))?;
    git(&["commit", "-m", "fixture"], Some(&work))?;

    let approved = "https://github.com/acme/site.git";
    git(&["remote", "add", "origin", approved], Some(&work))?;
    git(
        &[
            "remote",
            "set-url",
            "--push",
            "origin",
            redirected.to_str().ok_or("fixture path is not UTF-8")?,
        ],
        Some(&work),
    )?;
    let rewrite = format!(
        "url.file://{}.insteadOf",
        target.to_str().ok_or("fixture path is not UTF-8")?
    );
    git(&["config", "--local", &rewrite, approved], Some(&work))?;

    push_main(OsStr::new("git"), &work, approved).await?;

    let target_ref = Command::new("git")
        .arg(format!("--git-dir={}", target.display()))
        .args(["show-ref", "--verify", "refs/heads/main"])
        .output()?;
    assert!(target_ref.status.success());
    let redirected_ref = Command::new("git")
        .arg(format!("--git-dir={}", redirected.display()))
        .args(["show-ref", "--verify", "refs/heads/main"])
        .output()?;
    assert!(!redirected_ref.status.success());
    Ok(())
}

#[test]
fn missing_expected_request_fails_after_a_bounded_accept_deadline() -> TestResult {
    let server = Server::start(vec![(200, "[]".to_owned())])?;
    let started = Instant::now();
    assert!(server.finish().is_err());
    assert!(started.elapsed() < Duration::from_secs(2));
    Ok(())
}
