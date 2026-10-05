use super::*;

pub(crate) fn mock_sidecar(script: &str) -> SidecarProcess {
    use std::io::{BufReader, BufWriter};
    use std::process::{Command, Stdio};

    let mut child = Command::new("sh")
        .args(["-c", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    set_sidecar_nonblocking(stdout.as_raw_fd()).unwrap();
    SidecarProcess {
        writer: BufWriter::new(child.stdin.take().unwrap()),
        reader: BufReader::new(stdout),
        stderr: child.stderr.take(),
        child,
        next_id: 0,
        statement_timeout_ms: 0,
        request_timeout_ms: 100,
        startup_timeout_ms: 100,
    }
}

#[test]
fn consumes_buffered_response_after_idle_notification() {
    let mut sidecar = mock_sidecar(
        r#"read request
printf '%s\n' '{"type":"idle_disconnect"}' '{"id":0,"ok":"pong"}'
read next_request"#,
    );
    sidecar.ping().unwrap();
}

#[test]
fn drains_buffered_reply_after_notification_even_when_deadline_expires() {
    use std::io::BufRead;

    let mut sidecar = mock_sidecar(
        r#"printf '%s\n' '{"type":"idle_disconnect"}' '{"id":0,"ok":"pong"}'
read request"#,
    );
    wait_for_sidecar_output(sidecar.reader.get_ref().as_raw_fd(), Duration::from_secs(2)).unwrap();
    let buffered = sidecar.reader.fill_buf().unwrap();
    assert_eq!(buffered.iter().filter(|byte| **byte == b'\n').count(), 2);
    let deadline = Instant::now() - Duration::from_secs(1);
    assert_eq!(
        read_sidecar_line(&mut sidecar.reader, deadline, "ping").unwrap(),
        "{\"type\":\"idle_disconnect\"}\n"
    );
    assert_eq!(
        read_sidecar_line(&mut sidecar.reader, deadline, "ping").unwrap(),
        "{\"id\":0,\"ok\":\"pong\"}\n"
    );
    assert!(read_sidecar_line(&mut sidecar.reader, deadline, "ping").is_err());
    sidecar.force_kill_ref();
}

#[test]
fn startup_acknowledgement_has_a_deadline_and_kills_stalled_child() {
    let mut sidecar = mock_sidecar("read password; read request");
    let password = uuid::Uuid::new_v4().to_string();
    let start = std::time::Instant::now();
    let error = sidecar.send_password(&password, "jdbc").unwrap_err();
    assert!(error.to_string().contains("deadline for 'startup'"));
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
    assert!(sidecar.child.try_wait().unwrap().is_some());
}

#[test]
fn accepts_startup_acknowledgement_and_response_buffered_together() {
    let mut sidecar = mock_sidecar(
        r#"read password
printf '%s\n' 'ready' '{"id":0,"ok":"pong"}'
read request
read next_request"#,
    );
    let password = uuid::Uuid::new_v4().to_string();
    sidecar.send_password(&password, "jdbc").unwrap();
    sidecar.ping().unwrap();
}

#[test]
fn partial_response_has_a_deadline_and_invalidates_channel() {
    let mut sidecar = mock_sidecar(
        r#"read request
printf '%s' '{"id":0,"ok":'
read next_request"#,
    );
    let start = std::time::Instant::now();
    let error = sidecar.ping().unwrap_err();
    assert!(error.to_string().contains("deadline for 'ping'"));
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
    assert!(sidecar.child.try_wait().unwrap().is_some());
    assert!(sidecar.ping().is_err());
}

#[test]
fn notification_stream_does_not_extend_request_deadline() {
    let mut sidecar = mock_sidecar(
        r#"read request
while :; do printf '%s\n' '{"type":"idle_disconnect"}'; done"#,
    );
    let start = std::time::Instant::now();
    assert!(sidecar.ping().unwrap_err().to_string().contains("deadline"));
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
    assert!(sidecar.child.try_wait().unwrap().is_some());
}

#[test]
fn rejects_mismatched_response_id_and_invalidates_channel() {
    let mut sidecar = mock_sidecar(
        r#"read request
printf '%s\n' '{"id":999,"ok":"pong"}'
read next_request"#,
    );
    assert!(sidecar
        .ping()
        .unwrap_err()
        .to_string()
        .contains("mismatched response id"));
    assert!(sidecar.child.try_wait().unwrap().is_some());
}

#[test]
fn accepts_final_complete_response_when_child_exits() {
    let mut sidecar = mock_sidecar(
        r#"read request
printf '%s\n' '{"id":0,"ok":"pong"}'"#,
    );
    sidecar.ping().unwrap();
}

#[test]
fn rejects_truncated_response_at_eof_without_echoing_payload() {
    let mut sidecar = mock_sidecar(
        r#"read request
printf '%s' '{"id":0,"ok":"synthetic-private-payload"}'"#,
    );
    let error = sidecar.ping().unwrap_err().to_string();
    assert!(error.contains("incomplete response"));
    assert!(!error.contains("synthetic-private-payload"));
    assert!(sidecar.child.try_wait().unwrap().is_some());
}

#[test]
fn malformed_json_or_utf8_invalidates_channel_without_echoing_payload() {
    for output in [
        "printf '%s\\n' 'synthetic-invalid-json'",
        "printf '\\377\\n'",
    ] {
        let mut sidecar = mock_sidecar(&format!("read request\n{output}\nread next_request"));
        let error = sidecar.ping().unwrap_err().to_string();
        assert!(error.contains("sidecar returned invalid"));
        assert!(!error.contains("synthetic-invalid-json"));
        assert!(sidecar.child.try_wait().unwrap().is_some());
    }
}

#[test]
fn sql_error_keeps_valid_transport_available_for_next_request() {
    let mut sidecar = mock_sidecar(
        r#"read request
printf '%s\n' '{"id":0,"error":{"code":"SQL_ERROR","message":"synthetic syntax error"}}'
read ping
printf '%s\n' '{"id":1,"ok":"pong"}'
read next_request"#,
    );
    assert!(matches!(
        sidecar.execute("SELECT invalid"),
        Err(SafeselectError::SqlError(_))
    ));
    sidecar.ping().unwrap();
    assert!(sidecar.child.try_wait().unwrap().is_none());
}

#[test]
fn handles_interrupted_reads_and_rejects_invalid_descriptors() {
    let deadline = Instant::now() + Duration::from_secs(1);
    assert!(
        retry_sidecar_read(std::io::ErrorKind::Interrupted.into(), -1, deadline, "ping").is_ok()
    );
    assert!(retry_sidecar_read(
        std::io::ErrorKind::PermissionDenied.into(),
        -1,
        deadline,
        "ping"
    )
    .is_err());
    assert!(set_sidecar_nonblocking(-1).is_err());
    assert!(wait_for_sidecar_output(1_000_000, Duration::from_millis(1)).is_err());
}

#[test]
fn missing_or_rejected_startup_acknowledgement_terminates_child() {
    for script in [
        "read password",
        "read password; printf '%s\\n' 'rejected'; read request",
    ] {
        let mut sidecar = mock_sidecar(script);
        let password = uuid::Uuid::new_v4().to_string();
        assert!(sidecar.send_password(&password, "jdbc").is_err());
        assert!(sidecar.child.try_wait().unwrap().is_some());
    }
}
