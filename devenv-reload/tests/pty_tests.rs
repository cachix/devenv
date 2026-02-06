use devenv_reload::{CommandBuilder, Pty};
use portable_pty::PtySize;
use std::time::Duration;

const DEFAULT_SIZE: PtySize = PtySize {
    rows: 24,
    cols: 80,
    pixel_width: 0,
    pixel_height: 0,
};

#[test]
fn test_pty_spawn_echo_command() {
    let mut cmd = CommandBuilder::new("sh");
    cmd.arg("-c");
    cmd.arg("echo 'hello world'");

    let pty = Pty::spawn(cmd, DEFAULT_SIZE).expect("should spawn");

    let mut buf = [0u8; 1024];
    let mut output = Vec::new();

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        match pty.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                output.extend_from_slice(&buf[..n]);
                if String::from_utf8_lossy(&output).contains("hello world") {
                    break;
                }
            }
            Err(_) => break,
        }
    }

    let output_str = String::from_utf8_lossy(&output);
    assert!(
        output_str.contains("hello world"),
        "expected 'hello world' in output, got: {}",
        output_str
    );
}

#[test]
fn test_pty_spawn_shell_and_exit() {
    let mut cmd = CommandBuilder::new("sh");
    cmd.arg("-c");
    cmd.arg("exit 0");

    let pty = Pty::spawn(cmd, DEFAULT_SIZE).expect("should spawn");

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if let Ok(Some(status)) = pty.try_wait() {
            assert!(status.success());
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("shell did not exit in time");
}

#[test]
fn test_pty_write_to_stdin() {
    let cmd = CommandBuilder::new("cat");

    let pty = Pty::spawn(cmd, DEFAULT_SIZE).expect("should spawn");

    pty.write_all(b"test input\n")
        .expect("write should succeed");
    pty.flush().expect("flush should succeed");

    let mut buf = [0u8; 1024];
    let mut output = Vec::new();

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        match pty.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                output.extend_from_slice(&buf[..n]);
                if String::from_utf8_lossy(&output).contains("test input") {
                    break;
                }
            }
            Err(_) => break,
        }
    }

    let output_str = String::from_utf8_lossy(&output);
    assert!(
        output_str.contains("test input"),
        "expected 'test input' in output, got: {}",
        output_str
    );

    let _ = pty.kill();
}

#[test]
fn test_pty_kill() {
    let mut cmd = CommandBuilder::new("sleep");
    cmd.arg("60");

    let pty = Pty::spawn(cmd, DEFAULT_SIZE).expect("should spawn");

    // Verify process is running
    assert!(pty.try_wait().unwrap().is_none());

    // Kill it
    pty.kill().expect("kill should succeed");

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if pty.try_wait().unwrap().is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("process did not exit after kill");
}

#[test]
fn test_pty_resize() {
    let cmd = CommandBuilder::new("sh");

    let pty = Pty::spawn(cmd, DEFAULT_SIZE).expect("should spawn");

    let new_size = PtySize {
        rows: 40,
        cols: 120,
        pixel_width: 0,
        pixel_height: 0,
    };

    pty.resize(new_size).expect("resize should succeed");
}

#[test]
fn test_pty_spawn_nonexistent_command() {
    let cmd = CommandBuilder::new("/nonexistent/command/path");

    let result = Pty::spawn(cmd, DEFAULT_SIZE);
    assert!(result.is_err());
}

#[test]
fn test_pty_interactive_session() {
    let cmd = CommandBuilder::new("sh");

    let pty = Pty::spawn(cmd, DEFAULT_SIZE).expect("should spawn");

    // Send a command
    pty.write_all(b"echo $((1+1))\n").expect("write");
    pty.flush().expect("flush");

    let mut buf = [0u8; 1024];
    let mut output = Vec::new();

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        match pty.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                output.extend_from_slice(&buf[..n]);
                // Look for "2" in output (result of 1+1)
                if String::from_utf8_lossy(&output).contains("2") {
                    break;
                }
            }
            Err(_) => break,
        }
    }

    let output_str = String::from_utf8_lossy(&output);
    assert!(
        output_str.contains("2"),
        "expected '2' in output, got: {}",
        output_str
    );

    // Exit cleanly
    pty.write_all(b"exit\n").expect("write");
    let _ = pty.flush();

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if pty.try_wait().unwrap().is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
