//! Behavioral coverage for managed macOS process launch policy.

use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

use pretty_assertions::assert_eq;
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn managed_process_preserves_pid_and_uses_background_priority_when_available() {
    let command = super::managed_process(std::path::Path::new("/usr/bin/ruby"))
        .await
        .expect("managed command");
    let taskpolicy_available =
        command.get_program() == std::ffi::OsStr::new("/usr/sbin/taskpolicy");
    let mut command = tokio::process::Command::from(command);
    let mut child = command
        .args([
            "-e",
            "puts \"#{Process.pid} #{Process.getpriority(4, 0)}\"; STDOUT.flush; STDIN.read(1)",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start managed process");
    let spawned_pid = child.id().expect("managed process pid");

    child
        .stdin
        .take()
        .expect("managed process stdin")
        .write_all(b"x")
        .await
        .expect("release managed process");
    let output = child.wait_with_output().await.expect("managed process");

    assert!(output.status.success());
    let output = String::from_utf8(output.stdout).expect("utf-8 managed process output");
    let (pid, priority) = output.trim().split_once(' ').expect("pid and priority");
    assert_eq!(pid.parse::<u32>().expect("numeric pid"), spawned_pid);
    if taskpolicy_available {
        assert_eq!(priority, "1");
    }
}

#[tokio::test]
async fn timed_out_taskpolicy_probe_falls_back_to_direct_launch() {
    let mut probe = tokio::process::Command::new("/bin/sleep");
    probe.arg("60");
    let started = Instant::now();
    let command = super::managed_process_with_probe(
        std::path::Path::new("/usr/bin/ruby"),
        probe,
        Duration::from_millis(20),
    )
    .await
    .expect("managed command");

    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(command.get_program(), std::ffi::OsStr::new("/usr/bin/ruby"));

    let output = tokio::process::Command::from(command)
        .args(["-e", "puts 'ready'"])
        .output()
        .await
        .expect("direct launch");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"ready\n");
}
