//! Exercises the driver pipe end to end against a mock driver: handshake,
//! pose and input forwarding, log forwarding and reconnection.
//!
//! The mock speaks the same wire format as `driver_vrdummy` (docs/protocol.md)
//! but is independent of vrserver, so this runs in CI.

#![cfg(windows)]

use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use vrdummy_core::engine::Reply;
use vrdummy_core::{Config, Engine, win32};
use vrdummy_protocol::{
    Decoder, DeviceClass, DeviceInfo, HelloAck, LogLevel, LogRecord, Message, PROTOCOL_VERSION,
    encode,
};

/// Serves one client: answers Hello, forwards a log line, then relays every
/// decoded message until the client disconnects.
fn mock_driver_session(pipe_name: &str, received: &mpsc::Sender<Message>) {
    let file = win32::accept_named_pipe(pipe_name).expect("accept");
    let mut writer = file.try_clone().expect("clone");
    let mut reader = file;
    let mut decoder = Decoder::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        decoder.push(&buf[..n]);
        while let Some(frame) = decoder.next_frame() {
            let msg = frame.expect("valid frame");
            if let Message::Hello(h) = &msg {
                assert_eq!(h.protocol_version, u32::from(PROTOCOL_VERSION));
                let ack = Message::HelloAck(HelloAck {
                    protocol_version: u32::from(PROTOCOL_VERSION),
                    devices: vec![
                        DeviceInfo {
                            device_id: 0,
                            device_class: DeviceClass::Hmd,
                            serial: "MOCK-HMD".to_owned(),
                        },
                        DeviceInfo {
                            device_id: 1,
                            device_class: DeviceClass::ControllerLeft,
                            serial: "MOCK-L".to_owned(),
                        },
                    ],
                });
                writer.write_all(&encode(&ack)).expect("write ack");
                writer
                    .write_all(&encode(&Message::Log(LogRecord {
                        level: LogLevel::Info,
                        timestamp_us: 7,
                        text: "mock ready".to_owned(),
                    })))
                    .expect("write log");
                writer.flush().expect("flush");
            }
            let goodbye = matches!(msg, Message::Goodbye);
            received.send(msg).expect("forward");
            if goodbye {
                win32::disconnect_named_pipe(&writer);
                return;
            }
        }
    }
    win32::disconnect_named_pipe(&writer);
}

fn wait_for<F: FnMut() -> bool>(mut cond: F, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

fn collect_until<F: FnMut(&Message) -> bool>(
    rx: &mpsc::Receiver<Message>,
    mut pred: F,
    timeout: Duration,
) -> Vec<Message> {
    let mut got = Vec::new();
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(m) => {
                let done = pred(&m);
                got.push(m);
                if done {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    got
}

#[test]
fn handshake_sync_commands_and_reconnect() {
    let _ = vrdummy_core::logging::init(None, log::LevelFilter::Debug, true);
    let pipe_name = format!("vrdummy-test-driver-{}", std::process::id());
    let (tx, rx) = mpsc::channel();
    let server_name = pipe_name.clone();
    let server = std::thread::spawn(move || {
        // Two sessions: the first ends with Goodbye, the second with the drop.
        mock_driver_session(&server_name, &tx);
        mock_driver_session(&server_name, &tx);
    });

    let mut config = Config::default();
    config.driver.pipe_name = pipe_name;
    config.driver.reconnect_interval_ms = 50;
    let mut engine = Engine::new(config);

    assert!(
        wait_for(|| engine.try_connect(), Duration::from_secs(2)),
        "engine must connect to the mock driver"
    );
    assert!(engine.connected());
    assert_eq!(engine.driver_devices().len(), 2);
    assert_eq!(engine.driver_devices()[1].serial, "MOCK-L");

    // Initial sync: one pose per world device, then the display message.
    let initial = collect_until(
        &rx,
        |m| matches!(m, Message::Display(_)),
        Duration::from_secs(2),
    );
    let hello = initial
        .iter()
        .filter(|m| matches!(m, Message::Hello(_)))
        .count();
    let poses = initial
        .iter()
        .filter(|m| matches!(m, Message::Pose(_)))
        .count();
    assert_eq!(hello, 1);
    assert_eq!(poses, engine.world().device_ids().count());
    assert!(matches!(initial.last(), Some(Message::Display(_))));

    // Commands are forwarded with their values.
    assert!(matches!(
        engine.execute_line("pos hmd 0 1.9 -0.5"),
        Ok(Reply::Ok(_))
    ));
    assert!(matches!(
        engine.execute_line("input ctrl-l trigger/value 0.75"),
        Ok(Reply::Ok(_))
    ));
    assert!(matches!(engine.execute_line("ipd 0.07"), Ok(Reply::Ok(_))));
    let got = collect_until(
        &rx,
        |m| matches!(m, Message::Display(_)),
        Duration::from_secs(2),
    );
    let pose = got.iter().find_map(|m| match m {
        Message::Pose(p) if p.device_id == 0 => Some(p.clone()),
        _ => None,
    });
    assert_eq!(pose.map(|p| p.position), Some([0.0, 1.9, -0.5]));
    let input = got.iter().find_map(|m| match m {
        Message::Input(i) => Some(i.clone()),
        _ => None,
    });
    let input = input.expect("input forwarded");
    assert_eq!((input.device_id, input.component_id), (1, 5));
    assert_eq!(input.kind, vrdummy_protocol::InputKind::Scalar);
    assert!((input.value - 0.75).abs() < 1e-6);
    let display = got.iter().find_map(|m| match m {
        Message::Display(d) => Some(d.clone()),
        _ => None,
    });
    assert!((display.expect("display forwarded").ipd_m - 0.07).abs() < 1e-6);

    // Disconnecting sends Goodbye, which ends the first mock session. poll()
    // then reconnects to the second session and resends the kept state.
    engine.disconnect();
    assert!(!engine.connected());
    let first_session = collect_until(
        &rx,
        |m| matches!(m, Message::Goodbye),
        Duration::from_secs(2),
    );
    assert!(matches!(first_session.last(), Some(Message::Goodbye)));

    assert!(
        wait_for(
            || {
                engine.poll();
                engine.connected()
            },
            Duration::from_secs(3)
        ),
        "engine must reconnect to the second session"
    );
    let resynced = collect_until(
        &rx,
        |m| matches!(m, Message::Display(_)),
        Duration::from_secs(2),
    );
    let hmd_after = resynced.iter().find_map(|m| match m {
        Message::Pose(p) if p.device_id == 0 => Some(p.position),
        _ => None,
    });
    assert_eq!(hmd_after, Some([0.0, 1.9, -0.5]));

    // Driver log lines are drained by poll() without error.
    engine.poll();
    drop(engine);
    server.join().expect("mock driver thread");
}
