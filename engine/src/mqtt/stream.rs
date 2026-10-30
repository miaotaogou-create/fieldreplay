//! MQTT 底层流：明文 TCP 或 rustls 包装。

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, ServerConfig, ServerConnection, StreamOwned};

use super::tls;

pub enum MqttStream {
    Plain(TcpStream),
    TlsClient(StreamOwned<ClientConnection, TcpStream>),
    TlsServer(StreamOwned<ServerConnection, TcpStream>),
}

impl MqttStream {
    pub fn connect_plain(tcp: TcpStream) -> Self {
        Self::Plain(tcp)
    }

    pub fn connect_tls_client(
        tcp: TcpStream,
        client_config: Arc<ClientConfig>,
        server_name: ServerName<'static>,
    ) -> io::Result<Self> {
        let conn = ClientConnection::new(client_config, server_name).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("MQTT TLS 客户端连接创建失败: {e}"),
            )
        })?;
        // 握手在首次读写时完成；失败映射见上层 write/read 错误信息。
        Ok(Self::TlsClient(StreamOwned::new(conn, tcp)))
    }

    pub fn accept_tls_server(tcp: TcpStream, server_config: Arc<ServerConfig>) -> io::Result<Self> {
        let conn = ServerConnection::new(server_config).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("MQTT TLS 服务端连接创建失败: {e}"),
            )
        })?;
        Ok(Self::TlsServer(StreamOwned::new(conn, tcp)))
    }
}

impl Read for MqttStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Plain(s) => s.read(buf),
            Self::TlsClient(s) => map_tls_io(s.read(buf)),
            Self::TlsServer(s) => map_tls_io(s.read(buf)),
        }
    }
}

impl Write for MqttStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Plain(s) => s.write(buf),
            Self::TlsClient(s) => map_tls_io(s.write(buf)),
            Self::TlsServer(s) => map_tls_io(s.write(buf)),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Plain(s) => s.flush(),
            Self::TlsClient(s) => map_tls_io(s.flush()),
            Self::TlsServer(s) => map_tls_io(s.flush()),
        }
    }
}

fn map_tls_io<T>(r: io::Result<T>) -> io::Result<T> {
    r.map_err(|e| io::Error::new(e.kind(), format!("MQTT TLS I/O 失败: {e}")))
}

/// 按配置建立明文或 TLS 流（调用方已设好 TcpStream 超时）。
pub fn open_mqtt_stream(
    tcp: TcpStream,
    use_tls: bool,
    broker: &str,
    ca_cert_path: Option<&std::path::Path>,
    tls_server_name: Option<&str>,
) -> io::Result<MqttStream> {
    if !use_tls {
        return Ok(MqttStream::connect_plain(tcp));
    }
    let cfg = tls::build_client_config(ca_cert_path)?;
    let name = tls::server_name_for(broker, tls_server_name)?;
    MqttStream::connect_tls_client(tcp, cfg, name)
}
