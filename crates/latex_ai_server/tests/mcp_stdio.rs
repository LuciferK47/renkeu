//! `latex-ai mcp` over a real pipe: handshake, tool list, and the tools run
//! against a project.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::{Value, json};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

const DOC: &str = "\\documentclass{article}\n\
\\begin{document}\n\
First page text.\n\
\\newpage\n\
Second page text.\n\
\\end{document}\n";

struct Session {
    child: Child,
    stdin: ChildStdin,
    lines: tokio::io::Lines<BufReader<ChildStdout>>,
    next_id: u64,
    _dir: tempfile::TempDir,
}

impl Session {
    async fn start(files: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in files {
            std::fs::write(dir.path().join(name), text).unwrap();
        }
        let mut child = Command::new(env!("CARGO_BIN_EXE_latex-ai"))
            .arg("mcp")
            .arg(dir.path())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let mut s = Self {
            child,
            stdin,
            lines,
            next_id: 0,
            _dir: dir,
        };
        let init = s
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "test", "version": "0"}
                }),
            )
            .await;
        assert_eq!(init["result"]["serverInfo"]["name"], "latex-ai", "{init}");
        s.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .await;
        s
    }

    async fn send(&mut self, message: &Value) {
        let mut line = message.to_string();
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await.unwrap();
        self.stdin.flush().await.unwrap();
    }

    async fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await;
        loop {
            let line = tokio::time::timeout(Duration::from_secs(180), self.lines.next_line())
                .await
                .expect("the server answered in time")
                .unwrap()
                .expect("the server is still running");
            let message: Value = serde_json::from_str(&line)
                .unwrap_or_else(|e| panic!("stdout must carry only JSON-RPC, got {line:?}: {e}"));
            if message["id"] == id {
                return message;
            }
        }
    }

    /// Hang up the way a client does; the server must exit by itself.
    async fn finish(self) {
        let Self {
            mut child, stdin, ..
        } = self;
        drop(stdin);
        let status = tokio::time::timeout(Duration::from_secs(20), child.wait())
            .await
            .expect("the server exits when its client hangs up")
            .unwrap();
        assert!(status.success(), "{status}");
    }

    async fn call(&mut self, tool: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name": tool, "arguments": arguments}))
            .await["result"]
            .clone()
    }
}

#[tokio::test]
async fn the_tools_are_listed() {
    let mut s = Session::start(&[("main.tex", DOC)]).await;
    let list = s.request("tools/list", json!({})).await;
    let mut names: Vec<_> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "compile",
            "diagnostics",
            "layout_report",
            "locate_in_pdf",
            "locate_in_source",
            "render_page"
        ]
    );
    s.finish().await;
}

#[tokio::test]
async fn compile_layout_and_locate_work_on_a_project() {
    let mut s = Session::start(&[("main.tex", DOC)]).await;

    let compiled = s.call("compile", json!({})).await;
    assert_eq!(compiled["isError"], false, "{compiled}");
    assert_eq!(compiled["structuredContent"]["success"], true, "{compiled}");
    assert_eq!(compiled["structuredContent"]["pages"], 2, "{compiled}");

    let layout = s.call("layout_report", json!({"pages": 1})).await;
    assert_eq!(
        layout["structuredContent"]["layout"]["budget"]["over"], true,
        "{layout}"
    );

    let found = s
        .call("locate_in_pdf", json!({"file": "main.tex", "line": 5}))
        .await;
    assert_eq!(found["structuredContent"][0]["page"], 2, "{found}");

    let back = s
        .call(
            "locate_in_source",
            json!({"page": 2, "x": 120.0, "y": 110.0}),
        )
        .await;
    assert_eq!(back["structuredContent"]["file"], "main.tex", "{back}");
    s.finish().await;
}

#[tokio::test]
async fn a_broken_document_is_reported_to_the_agent_not_as_a_protocol_error() {
    let mut s = Session::start(&[(
        "main.tex",
        "\\documentclass{article}\n\\begin{document}\n\\nosuchcommand\n\\end{document}\n",
    )])
    .await;
    let compiled = s.call("compile", json!({})).await;
    assert_eq!(compiled["isError"], false, "{compiled}");
    assert_eq!(compiled["structuredContent"]["success"], false);
    assert!(
        compiled["structuredContent"]["first_error"]
            .as_str()
            .unwrap()
            .starts_with("main.tex:3"),
        "{compiled}"
    );

    let layout = s.call("layout_report", json!({})).await;
    assert_eq!(layout["isError"], true, "{layout}");
    s.finish().await;
}

#[tokio::test]
async fn a_bad_argument_comes_back_as_a_tool_error() {
    let mut s = Session::start(&[("main.tex", DOC)]).await;
    let out = s.call("diagnostics", json!({"severity": "loud"})).await;
    assert_eq!(out["isError"], true, "{out}");
    s.finish().await;
}

#[tokio::test]
async fn a_page_can_be_rendered_as_an_image() {
    let mut s = Session::start(&[("main.tex", DOC)]).await;
    let out = s.call("render_page", json!({"page": 1, "dpi": 50})).await;
    if out["isError"] == true {
        // Only acceptable when libpdfium cannot load (see the README).
        let why = out["content"][0]["text"].as_str().unwrap();
        assert!(why.contains("PDF rendering is unavailable"), "{why}");
    } else {
        let image = out["content"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["type"] == "image")
            .unwrap_or_else(|| panic!("no image in {out}"));
        assert_eq!(image["mimeType"], "image/png");
        assert!(image["data"].as_str().unwrap().len() > 100);
    }
    let missing = s.call("render_page", json!({"page": 9})).await;
    assert_eq!(missing["isError"], true, "{missing}");
    s.finish().await;
}
