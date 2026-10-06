//! Golden fixtures shared with the C++ driver tests.
//!
//! Each sample is encoded and compared byte for byte with
//! `tests/fixtures/<name>.bin` at the repository root. Setting
//! `VRDUMMY_UPDATE_FIXTURES=1` rewrites the files instead. The C++ test
//! `driver/tests/test_protocol.cpp` hardcodes the same values.

use std::path::PathBuf;

use vrdummy_protocol::{
    DecodeError, Decoder, DeviceClass, DeviceInfo, Display, DisplayFlags, HEADER_LEN, Hello,
    HelloAck, Input, InputKind, LogLevel, LogRecord, Message, PROTOCOL_VERSION, Pose, PoseFlags,
    TrackingResult, decode, encode, sizes,
};

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
}

fn samples() -> Vec<(&'static str, Message)> {
    vec![
        (
            "hello_v1",
            Message::Hello(Hello {
                protocol_version: 1,
                client_flags: 0,
            }),
        ),
        (
            "hello_ack_v1",
            Message::HelloAck(HelloAck {
                protocol_version: 1,
                devices: vec![
                    DeviceInfo {
                        device_id: 0,
                        device_class: DeviceClass::Hmd,
                        serial: "VRDUMMY-HMD-0".to_owned(),
                    },
                    DeviceInfo {
                        device_id: 1,
                        device_class: DeviceClass::ControllerLeft,
                        serial: "VRDUMMY-CTRL-L".to_owned(),
                    },
                    DeviceInfo {
                        device_id: 3,
                        device_class: DeviceClass::Tracker,
                        serial: "VRDUMMY-TRK-3".to_owned(),
                    },
                ],
            }),
        ),
        (
            "pose_update_v1",
            Message::Pose(Pose {
                device_id: 3,
                flags: PoseFlags::valid_connected(),
                tracking_result: TrackingResult::RunningOk,
                timestamp_us: 123_456_789,
                position: [0.1, 1.6, -0.25],
                rotation: [0.923_879_5, 0.0, 0.382_683_4, 0.0],
                linear_velocity: [0.5, 0.0, 0.0],
                angular_velocity: [0.0, 0.25, 0.0],
            }),
        ),
        (
            "input_update_v1",
            Message::Input(Input {
                device_id: 2,
                component_id: 11,
                kind: InputKind::Scalar,
                value: -0.5,
                time_offset_s: 0.0,
            }),
        ),
        (
            "display_update_v1",
            Message::Display(Display {
                flags: DisplayFlags(
                    DisplayFlags::IPD | DisplayFlags::PROJECTION | DisplayFlags::SWAP_EYES,
                ),
                ipd_m: 0.07,
                projection_left: [-1.2, 0.8, -1.0, 1.0],
                projection_right: [-0.8, 1.2, -1.0, 1.0],
                swap_eyes: true,
            }),
        ),
        (
            "log_record_v1",
            Message::Log(LogRecord {
                level: LogLevel::Info,
                timestamp_us: 42,
                text: "driver ready".to_owned(),
            }),
        ),
        ("goodbye_v1", Message::Goodbye),
    ]
}

#[test]
fn fixed_layout_sizes() {
    let cases = [
        (
            sizes::HELLO,
            Message::Hello(Hello {
                protocol_version: 1,
                client_flags: 0,
            }),
        ),
        (sizes::POSE, Message::Pose(Pose::identity(0))),
        (
            sizes::INPUT,
            Message::Input(Input {
                device_id: 0,
                component_id: 0,
                kind: InputKind::Bool,
                value: 0.0,
                time_offset_s: 0.0,
            }),
        ),
        (
            sizes::DISPLAY,
            Message::Display(Display {
                flags: DisplayFlags(0),
                ipd_m: 0.0,
                projection_left: [0.0; 4],
                projection_right: [0.0; 4],
                swap_eyes: false,
            }),
        ),
        (
            sizes::LOG_HEAD,
            Message::Log(LogRecord {
                level: LogLevel::Info,
                timestamp_us: 0,
                text: String::new(),
            }),
        ),
        (0, Message::Goodbye),
    ];
    for (expected, msg) in cases {
        assert_eq!(encode(&msg).len(), HEADER_LEN + expected, "{msg:?}");
    }
    let ack = Message::HelloAck(HelloAck {
        protocol_version: 1,
        devices: vec![DeviceInfo {
            device_id: 0,
            device_class: DeviceClass::Hmd,
            serial: String::new(),
        }],
    });
    assert_eq!(
        encode(&ack).len(),
        HEADER_LEN + sizes::HELLO_ACK_HEAD + sizes::DEVICE_INFO
    );
}

#[test]
fn round_trip() {
    for (name, msg) in samples() {
        let bytes = encode(&msg);
        let (decoded, consumed) = decode(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(consumed, bytes.len(), "{name}");
        assert_eq!(decoded, msg, "{name}");
        assert_eq!(u16::from_le_bytes([bytes[6], bytes[7]]), PROTOCOL_VERSION);
    }
}

#[test]
fn golden_fixtures() {
    let update = std::env::var_os("VRDUMMY_UPDATE_FIXTURES").is_some();
    let dir = fixture_dir();
    for (name, msg) in samples() {
        let path = dir.join(format!("{name}.bin"));
        let bytes = encode(&msg);
        if update {
            std::fs::create_dir_all(&dir).expect("create fixture dir");
            std::fs::write(&path, &bytes).expect("write fixture");
            continue;
        }
        let golden = std::fs::read(&path).unwrap_or_else(|e| {
            panic!(
                "{}: {e} (run with VRDUMMY_UPDATE_FIXTURES=1)",
                path.display()
            )
        });
        assert_eq!(golden, bytes, "{name}: encoding differs from fixture");
        let (decoded, _) = decode(&golden).expect("fixture decodes");
        assert_eq!(
            decoded, msg,
            "{name}: fixture decodes to a different message"
        );
    }
}

#[test]
fn decoder_resynchronises_after_bad_frames() {
    let good = encode(&Message::Goodbye);
    let mut bad_version = good.clone();
    bad_version[6] = 9;
    let mut unknown_type = good.clone();
    unknown_type[4] = 200;
    let mut malformed = encode(&Message::Hello(Hello {
        protocol_version: 1,
        client_flags: 0,
    }));
    malformed[0] = 7; // payload_len 7 instead of 8
    malformed.pop();

    let mut d = Decoder::new();
    d.push(&bad_version);
    d.push(&unknown_type);
    d.push(&malformed);
    d.push(&good[..5]);
    assert_eq!(d.next_frame(), Some(Err(DecodeError::VersionMismatch(9))));
    assert_eq!(d.next_frame(), Some(Err(DecodeError::UnknownType(200))));
    assert!(matches!(
        d.next_frame(),
        Some(Err(DecodeError::Malformed(_)))
    ));
    assert_eq!(d.next_frame(), None);
    d.push(&good[5..]);
    assert_eq!(d.next_frame(), Some(Ok(Message::Goodbye)));
    assert_eq!(d.next_frame(), None);
}

#[test]
fn oversized_payload_is_fatal() {
    let mut frame = vec![0u8; HEADER_LEN];
    frame[0..4].copy_from_slice(&(70_000u32).to_le_bytes());
    frame[4] = 7;
    frame[6] = 1;
    assert_eq!(decode(&frame), Err(DecodeError::PayloadTooLarge(70_000)));
}
