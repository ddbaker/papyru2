use std::{
    fs,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
};

use irpc::{channel::oneshot, rpc_requests};
use serde::{Deserialize, Serialize};

pub const QUIC_RPC_HOST: &str = "127.0.0.1";
pub const QUIC_RPC_PORT: u16 = 47473;

pub const RPC_CODE_OK: &str = "ok";
pub const RPC_CODE_INVALID_REQUEST: &str = "invalid_request";
pub const RPC_CODE_FILE_NOT_FOUND: &str = "file_not_found";
pub const RPC_CODE_INTERNAL_ERROR: &str = "internal_error";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PinFileRpcRequest {
    pub file_path: String,
    pub linenum: u32,
    pub platform: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PinFileRpcResponse {
    pub ok: bool,
    pub code: String,
    pub message: String,
    pub resolved_path: Option<String>,
}

impl PinFileRpcResponse {
    pub fn ok(path: &Path) -> Self {
        Self {
            ok: true,
            code: RPC_CODE_OK.to_string(),
            message: "file pinned".to_string(),
            resolved_path: Some(path.display().to_string()),
        }
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            code: RPC_CODE_INVALID_REQUEST.to_string(),
            message: message.into(),
            resolved_path: None,
        }
    }

    pub fn file_not_found(path: &Path) -> Self {
        Self {
            ok: false,
            code: RPC_CODE_FILE_NOT_FOUND.to_string(),
            message: format!("file not found: {}", path.display()),
            resolved_path: Some(path.display().to_string()),
        }
    }

    pub fn internal_error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            code: RPC_CODE_INTERNAL_ERROR.to_string(),
            message: message.into(),
            resolved_path: None,
        }
    }
}

#[rpc_requests(message = PinFileRpcMessage, no_spans)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PinFileRpcService {
    #[rpc(tx = oneshot::Sender<PinFileRpcResponse>)]
    PinFile(PinFileRpcRequest),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliPinTarget {
    pub file_path: String,
    pub linenum: u32,
}

pub fn quic_server_socket_addr() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), QUIC_RPC_PORT)
}

pub fn normalize_platform_tag(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "windows" => Some("windows"),
        "linux" => Some("linux"),
        "macos" => Some("macos"),
        _ => None,
    }
}

pub fn current_platform_tag() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

pub fn parse_cli_pin_target(raw: &str) -> Result<CliPinTarget, String> {
    let trimmed = raw.trim();
    let (path_part, line_part) = trimmed
        .rsplit_once(':')
        .ok_or_else(|| "argument must be '<relative_path>:<linenum>'".to_string())?;
    let file_path = path_part.trim();
    if file_path.is_empty() {
        return Err("file path must not be empty".to_string());
    }
    let linenum = line_part
        .trim()
        .parse::<u32>()
        .map_err(|_| "linenum must be a positive integer".to_string())?;
    if linenum == 0 {
        return Err("linenum must be >= 1".to_string());
    }
    Ok(CliPinTarget {
        file_path: file_path.to_string(),
        linenum,
    })
}

fn split_relative_components(raw: &str) -> Result<Vec<String>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("file_path must not be empty".to_string());
    }
    if trimmed.starts_with('/') || trimmed.starts_with('\\') {
        return Err("absolute file_path is not allowed".to_string());
    }
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return Err("drive-letter absolute file_path is not allowed".to_string());
    }

    let normalized = trimmed.replace('\\', "/");
    let mut components = Vec::new();
    for part in normalized.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err("path traversal is not allowed".to_string());
        }
        components.push(part.to_string());
    }
    if components.is_empty() {
        return Err("file_path must contain at least one path segment".to_string());
    }
    Ok(components)
}

pub fn resolve_request_file_path(
    user_document_dir: &Path,
    request_file_path: &str,
) -> Result<PathBuf, String> {
    let components = split_relative_components(request_file_path)?;
    let mut resolved = user_document_dir.to_path_buf();
    for component in components {
        resolved.push(component);
    }

    let user_doc_root =
        fs::canonicalize(user_document_dir).unwrap_or_else(|_| user_document_dir.to_path_buf());
    if resolved.exists() {
        let canonical = fs::canonicalize(&resolved)
            .map_err(|error| format!("failed to canonicalize request path: {error}"))?;
        if !canonical.starts_with(&user_doc_root) {
            return Err("resolved file_path escapes user_document_dir".to_string());
        }
    } else if !resolved.starts_with(user_document_dir) {
        return Err("resolved file_path escapes user_document_dir".to_string());
    }

    Ok(resolved)
}

pub fn content_line_count(content: &str) -> usize {
    content.split('\n').count().max(1)
}

pub fn clamp_linenum_1_based(requested: u32, total_lines: usize) -> u32 {
    let bounded_total = total_lines.max(1);
    let requested = requested.max(1) as usize;
    requested.min(bounded_total) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qproto_test1_parse_cli_target() {
        let parsed = parse_cli_pin_target(r#"2026\03\22\fileA.txt:16"#).expect("parse cli target");
        assert_eq!(parsed.file_path, r#"2026\03\22\fileA.txt"#);
        assert_eq!(parsed.linenum, 16);
    }

    #[test]
    fn qproto_test2_parse_cli_target_rejects_zero_line() {
        let error = parse_cli_pin_target("2026/03/22/fileA.txt:0")
            .expect_err("zero line should be rejected");
        assert!(error.contains(">= 1"));
    }

    #[test]
    fn qproto_test3_platform_normalization_case_insensitive() {
        assert_eq!(normalize_platform_tag("Windows"), Some("windows"));
        assert_eq!(normalize_platform_tag("LINUX"), Some("linux"));
        assert_eq!(normalize_platform_tag("mAcOs"), Some("macos"));
        assert_eq!(normalize_platform_tag("android"), None);
    }

    #[test]
    fn qproto_test4_resolve_request_file_path_accepts_relative_backslash_path() {
        let root = PathBuf::from("D:/tmp/user_document");
        let resolved = resolve_request_file_path(root.as_path(), r#"2026\03\22\fileA.txt"#)
            .expect("resolve relative backslash path");
        assert!(resolved.ends_with(Path::new("2026/03/22/fileA.txt")));
    }

    #[test]
    fn qproto_test5_resolve_request_file_path_rejects_traversal() {
        let root = PathBuf::from("D:/tmp/user_document");
        let error = resolve_request_file_path(root.as_path(), "../outside.txt")
            .expect_err("traversal path must be rejected");
        assert!(error.contains("traversal"));
    }

    #[test]
    fn qproto_test6_resolve_request_file_path_rejects_absolute() {
        let root = PathBuf::from("D:/tmp/user_document");
        let error = resolve_request_file_path(root.as_path(), "C:/abs/file.txt")
            .expect_err("absolute path must be rejected");
        assert!(error.contains("absolute"));
    }

    #[test]
    fn qproto_test7_clamp_linenum_is_1_based() {
        assert_eq!(clamp_linenum_1_based(0, 10), 1);
        assert_eq!(clamp_linenum_1_based(1, 10), 1);
        assert_eq!(clamp_linenum_1_based(9, 10), 9);
        assert_eq!(clamp_linenum_1_based(99, 10), 10);
        assert_eq!(clamp_linenum_1_based(3, 0), 1);
    }

    #[test]
    fn qproto_test8_content_line_count_uses_minimum_one_line() {
        assert_eq!(content_line_count(""), 1);
        assert_eq!(content_line_count("a"), 1);
        assert_eq!(content_line_count("a\nb\n"), 3);
    }

    #[test]
    fn qcli_test1_cli_arg_parser_accepts_relative_path_and_line() {
        let parsed =
            parse_cli_pin_target(r#"2026/03/22/fileA.txt:16"#).expect("parse cli target argument");
        assert_eq!(parsed.file_path, r#"2026/03/22/fileA.txt"#);
        assert_eq!(parsed.linenum, 16);
    }

    #[test]
    fn qcli_test2_cli_arg_parser_rejects_missing_colon_separator() {
        let error =
            parse_cli_pin_target("2026/03/22/fileA.txt").expect_err("missing ':' must be rejected");
        assert!(error.contains("<relative_path>:<linenum>"));
    }

    #[test]
    fn qcli_test3_cli_platform_inference_maps_to_supported_tag() {
        let tag = current_platform_tag();
        assert!(matches!(tag, "windows" | "linux" | "macos"));
        assert_eq!(normalize_platform_tag(tag), Some(tag));
    }

    #[test]
    fn qcli_test5_response_contract_contains_required_fields() {
        let response = PinFileRpcResponse::ok(Path::new("2026/03/22/fileA.txt"));
        let value = serde_json::to_value(response).expect("serialize response");
        assert!(value.get("ok").is_some());
        assert!(value.get("code").is_some());
        assert!(value.get("message").is_some());
        assert!(value.get("resolved_path").is_some());
    }
}
