//! MQTT TLS 辅助：客户端根证书、SNI；自检用服务端配置与嵌入证书。

use std::fs::File;
use std::io::{self, BufReader};
use std::path::Path;
use std::sync::Arc;

use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, RootCertStore, ServerConfig};

/// 自检嵌入 CA（仅信任该根时客户端用）。
pub const TEST_CA_PEM: &str = include_str!("testdata/ca.pem");
/// 自检嵌入服务端证书 + PKCS8 私钥（同 PEM 文件）。
pub const TEST_SERVER_PEM: &str = include_str!("testdata/server.pem");

pub fn build_client_config(ca_cert_path: Option<&Path>) -> io::Result<Arc<ClientConfig>> {
    let mut roots = RootCertStore::empty();
    if let Some(path) = ca_cert_path {
        let file = File::open(path).map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("打开 MQTT CA 证书失败 {}: {e}", path.display()),
            )
        })?;
        let mut reader = BufReader::new(file);
        let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("解析 MQTT CA PEM 失败 {}: {e}", path.display()),
                )
            })?;
        if certs.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("MQTT CA 文件无证书: {}", path.display()),
            ));
        }
        for cert in certs {
            roots.add(cert).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("加入 MQTT CA 失败: {e}"),
                )
            })?;
        }
    } else {
        let native = rustls_native_certs::load_native_certs();
        for cert in native.certs {
            let _ = roots.add(cert);
        }
        // 系统根为空或不完整时，用公共根兜底（公网 Broker）
        if roots.is_empty() {
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        }
    }

    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Arc::new(config))
}

pub fn server_name_for(host: &str, tls_server_name: Option<&str>) -> io::Result<ServerName<'static>> {
    let name = tls_server_name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(host);
    ServerName::try_from(name.to_string()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("无效 TLS 服务器名: {name}"),
        )
    })
}

/// 自检：用嵌入 PEM 建服务端 TLS 配置（无客户端认证）。
pub fn build_test_server_config() -> io::Result<Arc<ServerConfig>> {
    let mut reader = BufReader::new(TEST_SERVER_PEM.as_bytes());
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("解析自检服务端证书失败: {e}"),
            )
        })?;
    if certs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "自检服务端 PEM 无证书",
        ));
    }

    let mut key_reader = BufReader::new(TEST_SERVER_PEM.as_bytes());
    let key = rustls_pemfile::private_key(&mut key_reader)
        .map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("解析自检服务端私钥失败: {e}"),
            )
        })?
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "自检服务端 PEM 无私钥")
        })?;

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("构建自检 TLS ServerConfig 失败: {e}"),
            )
        })?;
    Ok(Arc::new(config))
}

/// 将嵌入 CA 写入临时文件，供客户端 `ca_cert_path` 自检。
pub fn write_test_ca_file(path: &Path) -> io::Result<()> {
    std::fs::write(path, TEST_CA_PEM)
}
