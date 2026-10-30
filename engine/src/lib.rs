//! FieldReplay 原生录放引擎库：`.fcap` 读写、UDP/视频(RTP·UDP|RTSP)/串口/TCP/MQTT 通道、录制与时间轴重放。

pub mod clock;
pub mod control;
pub mod fcap;
pub mod io_accept;
pub mod mqtt;
pub mod record;
pub mod replay;
pub mod rtsp;
pub mod serial;
pub mod tcp;
pub mod udp;

pub use clock::SessionClock;
pub use control::{serve as serve_control, API_VERSION as CONTROL_API_VERSION, DEFAULT_BIND};
pub use io_accept::run_io_acceptance;
pub use fcap::{
    ChannelEntry, ChannelType, FileHeader, PacketKind, PacketRecord, SessionReader, SessionWriter,
    FLAG_FINALIZED, FLAG_HAS_META, FCAP_MAGIC, FCAP_VERSION_V1, HEADER_BYTES,
};
pub use mqtt::{
    apply_topic_prefix, mqtt_qos_from_flags, mqtt_retain_from_flags, pack_mqtt_flags, MiniBroker,
    MqttClient, MqttConfig, FLAG_MQTT_QOS_MASK, FLAG_MQTT_RETAIN,
};
pub use rtsp::{MiniRtspServer, RtspClient, RtspUrl};
pub use record::{
    record_mqtt_to_file, record_mqtt_to_file_with, record_serial_to_file,
    record_serial_to_file_with, record_serial_with_port, record_serial_with_port_with,
    record_tcp_to_file, record_tcp_to_file_with, record_udp_to_file, record_udp_to_file_with,
    record_video_to_file, record_video_to_file_with, MqttRecordOptions, OnPacket, RecordOptions,
    SerialRecordOptions, TcpRecordOptions,
};
pub use replay::{
    apply_fault, replay_mqtt_from_file, replay_serial_from_file, replay_serial_with_port,
    replay_tcp_from_file, replay_udp_from_file, replay_video_from_file,
    replay_video_rtsp_from_file, FaultInjection, FaultRng, MqttReplayOptions, ReplayOptions,
    RtspReplayOptions, SerialReplayOptions, TcpReplayOptions,
};
pub use serial::{list_ports, Parity, SerialConfig, SerialPort};
pub use tcp::{TcpConfig, TcpRole};
