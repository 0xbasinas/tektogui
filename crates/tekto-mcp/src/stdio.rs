use std::io::{BufRead, Write};

use serde_json::{json, Value};

use super::{call_tool, tool_list, McpError};

// Bound memory use for both newline messages and legacy frames.
const MAX_MESSAGE: usize = 1024 * 1024;
const MAX_HEADER: usize = 4096;

#[derive(Clone, Copy)]
enum Framing {
    Newline,
    Legacy,
}

// Read and drain an entire line without retaining more than the limit.
fn line(r: &mut impl BufRead, limit: usize) -> Result<Option<(Vec<u8>, bool)>, McpError> {
    let mut result = Vec::new();
    let mut too_large = false;
    loop {
        let buf = r.fill_buf()?;
        if buf.is_empty() {
            return Ok(if result.is_empty() && !too_large {
                None
            } else {
                Some((result, too_large))
            });
        }
        let n = buf
            .iter()
            .position(|&b| b == b'\n')
            .map_or(buf.len(), |i| i + 1);
        let end = buf[n - 1] == b'\n';
        if result.len().saturating_add(n) > limit {
            too_large = true;
        } else if !too_large {
            result.extend_from_slice(&buf[..n]);
        }
        r.consume(n);
        if end {
            return Ok(Some((result, too_large)));
        }
    }
}

type Message = (Framing, Option<Vec<u8>>);

fn read_message(r: &mut impl BufRead) -> Result<Option<Message>, McpError> {
    let Some((first, oversized)) = line(r, MAX_MESSAGE)? else {
        return Ok(None);
    };
    if oversized {
        return Ok(Some((Framing::Newline, None)));
    }
    if first
        .get(..15)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"content-length:"))
    {
        if first.len() > MAX_HEADER {
            return Err(McpError::msg("stdio header too large"));
        }
        let value = std::str::from_utf8(&first[15..])
            .map_err(|_| McpError::msg("invalid content length"))?;
        let len: usize = value
            .trim()
            .parse()
            .map_err(|_| McpError::msg("invalid content length"))?;
        if len > MAX_MESSAGE {
            return Err(McpError::msg("stdio message too large"));
        }
        let mut header_len = first.len();
        loop {
            let Some((header, oversized)) = line(r, MAX_HEADER)? else {
                return Err(McpError::msg("eof in stdio header"));
            };
            header_len += header.len();
            if oversized || header_len > MAX_HEADER {
                return Err(McpError::msg("stdio header too large"));
            }
            if header == b"\r\n" || header == b"\n" {
                break;
            }
            if !header.ends_with(b"\n") {
                return Err(McpError::msg("eof in stdio header"));
            }
        }
        let mut body = vec![0; len];
        r.read_exact(&mut body)?;
        Ok(Some((Framing::Legacy, Some(body))))
    } else {
        Ok(Some((Framing::Newline, Some(first))))
    }
}

fn write_message(w: &mut impl Write, framing: Framing, value: &Value) -> Result<(), McpError> {
    let body = serde_json::to_vec(value)?;
    match framing {
        Framing::Newline => {
            w.write_all(&body)?;
            w.write_all(b"\n")?;
        }
        Framing::Legacy => {
            write!(w, "Content-Length: {}\r\n\r\n", body.len())?;
            w.write_all(&body)?;
        }
    }
    w.flush()?;
    Ok(())
}

pub(super) fn run(mut input: impl BufRead, mut output: impl Write) -> Result<(), McpError> {
    while let Some((framing, message)) = read_message(&mut input)? {
        let parsed = message
            .as_deref()
            .and_then(|m| serde_json::from_slice::<Value>(m).ok());
        let Some(v) = parsed else {
            write_message(
                &mut output,
                framing,
                &json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}}),
            )?;
            continue;
        };
        let method = v.get("method").and_then(Value::as_str);
        if !v.is_object()
            || v.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
            || method.is_none()
        {
            write_message(
                &mut output,
                framing,
                &json!({"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid Request"}}),
            )?;
            continue;
        }
        let id = v.get("id").cloned();
        let result = match method.unwrap() {
            "notifications/initialized" => Ok(json!({})),
            "initialize" => Ok(
                json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"tekto","version":"0.1.0"}}),
            ),
            "tools/list" => Ok(json!({"tools":tool_list()})),
            "tools/call" => {
                let params = v.get("params").cloned().unwrap_or(json!({}));
                Ok(call_tool(&params).unwrap_or_else(
                    |e| json!({"content":[{"type":"text","text":e.to_string()}],"isError":true}),
                ))
            }
            "ping" => Ok(json!({})),
            _ => Err(()),
        };
        if let Some(id) = id {
            let response = match result {
                Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
                Err(()) => {
                    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"method not found"}})
                }
            };
            write_message(&mut output, framing, &response)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn exchange(input: &[u8]) -> Vec<u8> {
        let mut output = Vec::new();
        run(Cursor::new(input), &mut output).unwrap();
        output
    }

    #[test]
    fn newline_recovery_crlf_eof_and_notifications() {
        let input = b"{bad}\n{\"jsonrpc\":\"2.0\",\"method\":\"unknown\"}\r\n{\"jsonrpc\":\"2.0\",\"method\":\"ping\"}\n{\"jsonrpc\":\"2.0\",\"method\":\"ping\",\"id\":1}\r\n{\"jsonrpc\":\"2.0\",\"method\":\"ping\",\"id\":2}";
        let output = exchange(input);
        let responses: Vec<Value> = output
            .split(|&b| b == b'\n')
            .filter(|s| !s.is_empty())
            .map(|s| serde_json::from_slice(s).unwrap())
            .collect();
        assert_eq!(responses.len(), 3);
        assert_eq!(responses[0]["error"]["code"], -32700);
        assert_eq!(responses[1]["id"], 1);
        assert_eq!(responses[2]["id"], 2);
        assert!(exchange(b"").is_empty());
    }

    #[test]
    fn legacy_frames_recover_and_match_response_framing() {
        let request = br#"{"jsonrpc":"2.0","method":"initialize","id":7}"#;
        let input = format!(
            "Content-Length: 1\r\n\r\n{{Content-Length: {}\r\n\r\n{}",
            request.len(),
            std::str::from_utf8(request).unwrap()
        );
        let output = exchange(input.as_bytes());
        let mut reader = Cursor::new(output);
        let (_, first) = read_message(&mut reader).unwrap().unwrap();
        let (_, second) = read_message(&mut reader).unwrap().unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&first.unwrap()).unwrap()["error"]["code"],
            -32700
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&second.unwrap()).unwrap()["result"]["protocolVersion"],
            "2024-11-05"
        );
        assert!(read_message(&mut reader).unwrap().is_none());
    }

    #[test]
    fn bounds_and_truncated_frames() {
        let mut input = vec![b'x'; MAX_MESSAGE + 1];
        input.extend_from_slice(b"\n{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"ping\"}\n");
        let output = exchange(&input);
        let responses: Vec<Value> = output
            .split(|&b| b == b'\n')
            .filter(|s| !s.is_empty())
            .map(|s| serde_json::from_slice(s).unwrap())
            .collect();
        assert_eq!(responses[0]["error"]["code"], -32700);
        assert_eq!(responses[1]["id"], 3);
        assert!(run(
            Cursor::new(format!("Content-Length: {}\r\n\r\n", MAX_MESSAGE + 1)),
            Vec::new()
        )
        .is_err());
        assert!(run(Cursor::new(b"Content-Length: 5\r\n\r\n{}"), Vec::new()).is_err());
        assert!(run(
            Cursor::new(format!(
                "Content-Length: 1\r\n{}\r\n\r\n",
                "x".repeat(MAX_HEADER)
            )),
            Vec::new()
        )
        .is_err());
    }
}
