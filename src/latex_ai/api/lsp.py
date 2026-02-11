"""LSP WebSocket router."""
from __future__ import annotations

import logging
from pathlib import Path
from typing import List

from fastapi import APIRouter, WebSocket, WebSocketDisconnect

from latex_ai.state import state

router = APIRouter()

try:
    from latex_ai.lsp_client import TexLabClient

    LSP_AVAILABLE = True
except ImportError:
    TexLabClient = None  # type: ignore
    LSP_AVAILABLE = False


@router.websocket("/ws/lsp")
async def lsp_websocket(websocket: WebSocket, workspace: str | None = None):
    await websocket.accept()
    if not LSP_AVAILABLE:
        await websocket.send_json(
            {
                "type": "info",
                "message": "LSP features not available. To enable: cargo install texlab",
                "optional": True,
            }
        )
        await websocket.close()
        return

    workspace_path = Path(workspace) if workspace else Path.cwd()
    workspace_key = str(workspace_path.resolve())

    if workspace_key not in state.lsp_clients:
        try:
            client = TexLabClient(workspace_path)
            await client.start()
            state.lsp_clients[workspace_key] = client
        except Exception:
            await websocket.send_json(
                {
                    "type": "info",
                    "message": "TexLab not installed. LSP features disabled. Install with: cargo install texlab",
                    "optional": True,
                }
            )
            await websocket.close()
            return

    client = state.lsp_clients[workspace_key]

    async def on_diagnostics(uri: str, diagnostics: List):
        try:
            await websocket.send_json(
                {
                    "type": "diagnostics",
                    "uri": uri,
                    "diagnostics": [
                        {
                            "range": d.range,
                            "severity": d.severity,
                            "message": d.message,
                            "source": d.source,
                            "code": d.code,
                        }
                        for d in diagnostics
                    ],
                }
            )
        except Exception as exc:
            logging.error(f"Error sending diagnostics: {exc}")

    client.on_diagnostics = on_diagnostics

    try:
        while True:
            data = await websocket.receive_json()
            method = data.get("method")
            params = data.get("params", {})
            try:
                if method == "textDocument/didOpen":
                    file_path = Path(params["textDocument"]["uri"].replace("file://", ""))
                    content = params["textDocument"]["text"]
                    await client.did_open(file_path, content)
                    await websocket.send_json({"type": "ack", "method": method})
                elif method == "textDocument/didChange":
                    file_path = Path(params["textDocument"]["uri"].replace("file://", ""))
                    content = params["contentChanges"][0]["text"]
                    version = params["textDocument"].get("version", 1)
                    await client.did_change(file_path, content, version)
                    await websocket.send_json({"type": "ack", "method": method})
                elif method == "textDocument/didClose":
                    file_path = Path(params["textDocument"]["uri"].replace("file://", ""))
                    await client.did_close(file_path)
                    await websocket.send_json({"type": "ack", "method": method})
                elif method == "textDocument/completion":
                    file_path = Path(params["textDocument"]["uri"].replace("file://", ""))
                    position = params["position"]
                    items = await client.get_completion(file_path, position["line"], position["character"])
                    await websocket.send_json({"type": "response", "method": method, "result": items})
                elif method == "textDocument/hover":
                    file_path = Path(params["textDocument"]["uri"].replace("file://", ""))
                    position = params["position"]
                    hover = await client.get_hover(file_path, position["line"], position["character"])
                    await websocket.send_json({"type": "response", "method": method, "result": hover})
                elif method == "textDocument/definition":
                    file_path = Path(params["textDocument"]["uri"].replace("file://", ""))
                    position = params["position"]
                    definitions = await client.get_definition(file_path, position["line"], position["character"])
                    await websocket.send_json({"type": "response", "method": method, "result": definitions})
                elif method == "textDocument/formatting":
                    file_path = Path(params["textDocument"]["uri"].replace("file://", ""))
                    edits = await client.format_document(file_path)
                    await websocket.send_json({"type": "response", "method": method, "result": edits})
                else:
                    await websocket.send_json({"type": "error", "message": f"Unsupported method: {method}"})
            except Exception as exc:
                logging.error(f"LSP method error: {exc}")
                await websocket.send_json({"type": "error", "method": method, "message": str(exc)})
    except WebSocketDisconnect:
        logging.info("LSP WebSocket disconnected")
    except Exception as exc:
        logging.error(f"LSP WebSocket error: {exc}")
    finally:
        pass
