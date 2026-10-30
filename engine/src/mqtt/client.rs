//! MQTT 客户端：连 Broker、订阅收消息、按时间轴发布。

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::codec::{
    encode_connect, encode_disconnect, encode_pingreq, encode_puback, encode_pubcomp,
    encode_publish, encode_pubrel, encode_subscribe, decode_packet, Incoming, Publish,
};
use super::stream::{open_mqtt_stream, MqttStream};
use super::MqttConfig;

pub struct MqttClient {
    stream: MqttStream,
    buf: Vec<u8>,
    next_packet_id: u16,
}

impl MqttClient {
    pub fn connect(config: &MqttConfig, read_timeout: Duration) -> io::Result<Self> {
        let addr = config.socket_addr()?;
        let tcp = TcpStream::connect(addr)?;
        tcp.set_nodelay(true)?;
        tcp.set_read_timeout(Some(read_timeout))?;
        tcp.set_write_timeout(Some(Duration::from_secs(5)))?;

        let stream = open_mqtt_stream(
            tcp,
            config.tls,
            &config.broker,
            config.ca_path(),
            config.tls_server_name.as_deref(),
        )?;

        let client_id = format!("fieldreplay-{}", std::process::id());
        let mut client = Self {
            stream,
            buf: Vec::with_capacity(4096),
            next_packet_id: 1,
        };
        let user = config.username.as_deref();
        let pass = config.password.as_deref().map(|s| s.as_bytes());
        client.write_all(&encode_connect(&client_id, 60, true, user, pass))?;
        match client.wait_packet(|p| matches!(p, Incoming::Connack { .. }), Duration::from_secs(5))?
        {
            Incoming::Connack { return_code, .. } if return_code == 0 => {}
            Incoming::Connack { return_code, .. } => {
                let reason = match return_code {
                    1 => "不可接受的协议版本",
                    2 => "标识符被拒绝",
                    3 => "服务器不可用",
                    4 => "用户名或密码错误",
                    5 => "未授权",
                    _ => "未知原因",
                };
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionRefused,
                    format!("MQTT CONNACK 拒绝: return_code={return_code} ({reason})"),
                ));
            }
            _ => unreachable!(),
        }
        Ok(client)
    }

    pub fn subscribe(&mut self, topic: &str, qos: u8) -> io::Result<()> {
        let id = self.alloc_id();
        self.write_all(&encode_subscribe(id, topic, qos.min(2)))?;
        match self.wait_packet(
            |p| matches!(p, Incoming::Suback { packet_id, .. } if *packet_id == id),
            Duration::from_secs(5),
        )? {
            Incoming::Suback { codes, .. } => {
                if codes.iter().any(|&c| c == 0x80) {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "MQTT SUBACK 失败",
                    ));
                }
                Ok(())
            }
            _ => unreachable!(),
        }
    }

    pub fn publish(&mut self, topic: &str, payload: &[u8], qos: u8, retain: bool) -> io::Result<()> {
        let qos = qos.min(2);
        let packet_id = if qos > 0 {
            Some(self.alloc_id())
        } else {
            None
        };
        let msg = Publish {
            dup: false,
            qos,
            retain,
            topic: topic.to_string(),
            packet_id,
            payload: payload.to_vec(),
        };
        self.write_all(&encode_publish(&msg)?)?;
        match qos {
            0 => Ok(()),
            1 => {
                let id = packet_id.unwrap();
                let _ = self.wait_packet(
                    |p| matches!(p, Incoming::Puback(pid) if *pid == id),
                    Duration::from_secs(5),
                )?;
                Ok(())
            }
            2 => {
                // ponytail: 完整 QoS2 握手；上限是无本地持久化，断线后可能重复。升级：会话状态落盘。
                let id = packet_id.unwrap();
                let _ = self.wait_packet(
                    |p| matches!(p, Incoming::Pubrec(pid) if *pid == id),
                    Duration::from_secs(5),
                )?;
                self.write_all(&encode_pubrel(id))?;
                let _ = self.wait_packet(
                    |p| matches!(p, Incoming::Pubcomp(pid) if *pid == id),
                    Duration::from_secs(5),
                )?;
                Ok(())
            }
            _ => unreachable!(),
        }
    }

    /// 轮询一条入站 PUBLISH；顺带处理 QoS 应答与 PING。
    /// `Ok(None)` = 超时无数据。
    pub fn poll_publish(&mut self, stop: &AtomicBool) -> io::Result<Option<Publish>> {
        while !stop.load(Ordering::SeqCst) {
            match self.read_next_packet()? {
                None => return Ok(None),
                Some(Incoming::Publish(msg)) => {
                    self.ack_incoming_publish(&msg)?;
                    return Ok(Some(msg));
                }
                Some(Incoming::Pingreq) => {
                    // 客户端不应收到；忽略
                }
                Some(Incoming::Pingresp) | Some(Incoming::Puback(_)) | Some(Incoming::Pubcomp(_)) => {}
                Some(Incoming::Pubrec(id)) => {
                    self.write_all(&encode_pubrel(id))?;
                }
                Some(Incoming::Pubrel(id)) => {
                    self.write_all(&encode_pubcomp(id))?;
                }
                Some(Incoming::Disconnect) => {
                    return Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        "Broker 断开 MQTT 连接",
                    ));
                }
                Some(_) => {}
            }
        }
        Ok(None)
    }

    pub fn disconnect(mut self) -> io::Result<()> {
        let _ = self.write_all(&encode_disconnect());
        Ok(())
    }

    pub fn ping(&mut self) -> io::Result<()> {
        self.write_all(&encode_pingreq())
    }

    fn ack_incoming_publish(&mut self, msg: &Publish) -> io::Result<()> {
        match msg.qos {
            0 => Ok(()),
            1 => {
                let id = msg.packet_id.ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "QoS1 PUBLISH 缺 packet_id")
                })?;
                self.write_all(&encode_puback(id))
            }
            2 => {
                let id = msg.packet_id.ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "QoS2 PUBLISH 缺 packet_id")
                })?;
                self.write_all(&crate::mqtt::codec::encode_pubrec(id))?;
                // 等 PUBREL 在后续 poll 里处理；此处先返回消息给录制侧
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn alloc_id(&mut self) -> u16 {
        let id = self.next_packet_id;
        self.next_packet_id = self.next_packet_id.wrapping_add(1);
        if self.next_packet_id == 0 {
            self.next_packet_id = 1;
        }
        id
    }

    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        self.stream.write_all(data)?;
        Ok(())
    }

    fn read_next_packet(&mut self) -> io::Result<Option<Incoming>> {
        loop {
            if let Some((n, pkt)) = decode_packet(&self.buf)? {
                self.buf.drain(..n);
                return Ok(Some(pkt));
            }
            let mut tmp = [0u8; 4096];
            match self.stream.read(&mut tmp) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "MQTT TCP 连接关闭",
                    ));
                }
                Ok(n) => self.buf.extend_from_slice(&tmp[..n]),
                Err(e)
                    if e.kind() == io::ErrorKind::WouldBlock
                        || e.kind() == io::ErrorKind::TimedOut =>
                {
                    return Ok(None);
                }
                Err(e) => return Err(e),
            }
        }
    }

    fn wait_packet(
        &mut self,
        mut pred: impl FnMut(&Incoming) -> bool,
        timeout: Duration,
    ) -> io::Result<Incoming> {
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            match self.read_next_packet()? {
                Some(pkt) if pred(&pkt) => return Ok(pkt),
                Some(Incoming::Publish(msg)) => {
                    // 握手期间偶发入站：先应答，丢弃载荷（订阅建立前）
                    self.ack_incoming_publish(&msg)?;
                }
                Some(Incoming::Pingreq) => {}
                Some(Incoming::Pubrel(id)) => {
                    self.write_all(&encode_pubcomp(id))?;
                }
                Some(_) => {}
                None => continue,
            }
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "等待 MQTT 应答超时",
        ))
    }
}
