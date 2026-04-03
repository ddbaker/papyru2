use std::{
    env,
    fs::OpenOptions,
    io::Write,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, anyhow};
use papyru2::{path_resolver, quic_rpc_protocol};

fn append_cli_log(app_paths: &path_resolver::AppPaths, message: impl AsRef<str>) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let line = format!("[{now}] {}\n", message.as_ref());
    let path = app_paths.log_file_path("papyru2_pin_file.log");
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
    }
}

fn send_pin_request(
    request: quic_rpc_protocol::PinFileRpcRequest,
) -> anyhow::Result<quic_rpc_protocol::PinFileRpcResponse> {
    send_pin_request_to_addr(request, quic_rpc_protocol::quic_server_socket_addr())
}

fn send_pin_request_to_addr(
    request: quic_rpc_protocol::PinFileRpcRequest,
    server_addr: SocketAddr,
) -> anyhow::Result<quic_rpc_protocol::PinFileRpcResponse> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to create tokio runtime for CLI request")?;
    runtime.block_on(async move {
        let endpoint = irpc::util::make_insecure_client_endpoint(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            0,
        ))
        .map_err(|error| anyhow!("failed to create insecure client endpoint: {error}"))?;
        let client =
            irpc::Client::<quic_rpc_protocol::PinFileRpcService>::noq(endpoint, server_addr);
        client
            .rpc(request)
            .await
            .map_err(|error| anyhow!("rpc call failed: {error}"))
    })
}

fn main() {
    let app_paths = match path_resolver::AppPaths::resolve() {
        Ok(paths) => paths,
        Err(error) => {
            eprintln!("papyru2_pin_file: path resolver failed: {error}");
            process::exit(2);
        }
    };

    let Some(raw_target) = env::args().nth(1) else {
        eprintln!("usage: papyru2_pin_file \"<relative_path>:<linenum>\"");
        process::exit(2);
    };

    append_cli_log(
        &app_paths,
        format!(
            "request start target='{}' server={} ",
            raw_target,
            quic_rpc_protocol::quic_server_socket_addr()
        ),
    );

    let parsed = match quic_rpc_protocol::parse_cli_pin_target(raw_target.as_str()) {
        Ok(parsed) => parsed,
        Err(error) => {
            let response = quic_rpc_protocol::PinFileRpcResponse::invalid_request(error);
            let json = serde_json::to_string(&response)
                .unwrap_or_else(|_| "{\"ok\":false,\"code\":\"internal_error\",\"message\":\"serialization failed\",\"resolved_path\":null}".to_string());
            println!("{json}");
            append_cli_log(
                &app_paths,
                format!("request rejected by cli validation target='{}'", raw_target),
            );
            process::exit(2);
        }
    };

    let request = quic_rpc_protocol::PinFileRpcRequest {
        file_path: parsed.file_path,
        linenum: parsed.linenum,
        platform: quic_rpc_protocol::current_platform_tag().to_string(),
    };
    append_cli_log(
        &app_paths,
        format!(
            "request send file_path='{}' linenum={} platform='{}'",
            request.file_path, request.linenum, request.platform
        ),
    );

    let response = match send_pin_request(request) {
        Ok(response) => response,
        Err(error) => {
            let response = quic_rpc_protocol::PinFileRpcResponse::internal_error(error.to_string());
            let json = serde_json::to_string(&response)
                .unwrap_or_else(|_| "{\"ok\":false,\"code\":\"internal_error\",\"message\":\"serialization failed\",\"resolved_path\":null}".to_string());
            println!("{json}");
            append_cli_log(
                &app_paths,
                format!("request transport failure error={error}"),
            );
            process::exit(1);
        }
    };

    let json = serde_json::to_string(&response).unwrap_or_else(|_| {
        "{\"ok\":false,\"code\":\"internal_error\",\"message\":\"serialization failed\",\"resolved_path\":null}".to_string()
    });
    println!("{json}");
    append_cli_log(
        &app_paths,
        format!(
            "request done ok={} code={} resolved_path={}",
            response.ok,
            response.code,
            response.resolved_path.as_deref().unwrap_or("<none>")
        ),
    );

    if response.ok {
        process::exit(0);
    }
    process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn qcli_test4_server_unavailable_path_returns_error() {
        let request = quic_rpc_protocol::PinFileRpcRequest {
            file_path: "2026/03/22/fileA.txt".to_string(),
            linenum: 16,
            platform: quic_rpc_protocol::current_platform_tag().to_string(),
        };
        let closed_local_port = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 1);
        let error = send_pin_request_to_addr(request, closed_local_port)
            .expect_err("request against closed localhost port must fail");
        assert!(error.to_string().contains("rpc call failed"));
    }

    #[test]
    fn qcli_test5_cli_log_output_is_written_under_log_dir() {
        let app_paths = path_resolver::AppPaths::resolve().expect("resolve app paths");
        let marker = format!(
            "qcli_test5_marker_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        );
        append_cli_log(&app_paths, marker.as_str());

        let log_path = app_paths.log_file_path("papyru2_pin_file.log");
        let text = fs::read_to_string(log_path).expect("read cli log file");
        assert!(text.contains(marker.as_str()));
    }
}
