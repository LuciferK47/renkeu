"""LaTeX Language Server Protocol client for TexLab integration."""

import asyncio
import json
import logging
import subprocess
from pathlib import Path
from typing import Optional, Dict, List, Any, Callable
from dataclasses import dataclass, field
from enum import Enum


class MessageType(Enum):
    """LSP message types."""
    REQUEST = "request"
    RESPONSE = "response"
    NOTIFICATION = "notification"
    ERROR = "error"


@dataclass
class LSPMessage:
    """LSP JSON-RPC message."""
    jsonrpc: str = "2.0"
    id: Optional[int] = None
    method: Optional[str] = None
    params: Optional[Dict] = None
    result: Optional[Any] = None
    error: Optional[Dict] = None


@dataclass
class DiagnosticSeverity(Enum):
    """Diagnostic severity levels."""
    ERROR = 1
    WARNING = 2
    INFORMATION = 3
    HINT = 4


@dataclass
class Diagnostic:
    """LSP diagnostic (error, warning, etc.)."""
    range: Dict  # {start: {line, character}, end: {line, character}}
    severity: int
    message: str
    source: str = "texlab"
    code: Optional[str] = None


class TexLabClient:
    """Client for TexLab LaTeX Language Server."""
    
    def __init__(self, workspace_root: Path):
        """Initialize TexLab client.
        
        Args:
            workspace_root: Root directory of LaTeX project
        """
        self.workspace_root = workspace_root
        self.process: Optional[subprocess.Popen] = None
        self.request_id = 0
        self.pending_requests: Dict[int, asyncio.Future] = {}
        self.initialized = False
        self.diagnostics: Dict[str, List[Diagnostic]] = {}  # file_uri -> diagnostics
        
        # Callbacks
        self.on_diagnostics: Optional[Callable[[str, List[Diagnostic]], None]] = None
        
    async def start(self):
        """Start the TexLab server process."""
        try:
            # Check if texlab is available
            result = await asyncio.create_subprocess_exec(
                "texlab", "--version",
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE
            )
            await result.wait()
            
            if result.returncode != 0:
                raise RuntimeError("TexLab not found. Install with: cargo install texlab")
            
            # Start texlab server
            self.process = await asyncio.create_subprocess_exec(
                "texlab",
                stdin=asyncio.subprocess.PIPE,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE
            )
            
            # Start message handlers
            asyncio.create_task(self._read_messages())
            
            # Initialize the server
            await self._initialize()
            
            logging.info("TexLab LSP server started")
            
        except Exception as e:
            logging.error(f"Failed to start TexLab: {e}")
            raise
    
    async def stop(self):
        """Stop the TexLab server."""
        if self.process:
            try:
                # Send shutdown request
                await self._send_request("shutdown", {})
                
                # Send exit notification
                await self._send_notification("exit", {})
                
                # Wait for process to exit
                await asyncio.wait_for(self.process.wait(), timeout=5.0)
                
            except asyncio.TimeoutError:
                # Force kill if it doesn't exit gracefully
                self.process.kill()
                await self.process.wait()
            
            self.process = None
            logging.info("TexLab LSP server stopped")
    
    async def _initialize(self):
        """Send initialize request to LSP server."""
        init_params = {
            "processId": None,
            "rootUri": f"file://{self.workspace_root.as_posix()}",
            "capabilities": {
                "textDocument": {
                    "publishDiagnostics": {},
                    "completion": {"completionItem": {"snippetSupport": True}},
                    "hover": {"contentFormat": ["markdown", "plaintext"]},
                    "definition": {},
                    "references": {},
                    "documentSymbol": {},
                    "formatting": {}
                },
                "workspace": {
                    "workspaceFolders": True,
                    "configuration": True
                }
            },
            "workspaceFolders": [
                {
                    "uri": f"file://{self.workspace_root.as_posix()}",
                    "name": self.workspace_root.name
                }
            ]
        }
        
        result = await self._send_request("initialize", init_params)
        
        # Send initialized notification
        await self._send_notification("initialized", {})
        
        self.initialized = True
        logging.info("TexLab initialized successfully")
    
    async def did_open(self, file_path: Path, content: str, language_id: str = "latex"):
        """Notify server that a document was opened.
        
        Args:
            file_path: Path to the document
            content: Document content
            language_id: Language identifier (default: latex)
        """
        if not self.initialized:
            raise RuntimeError("TexLab not initialized")
        
        uri = f"file://{file_path.as_posix()}"
        await self._send_notification("textDocument/didOpen", {
            "textDocument": {
                "uri": uri,
                "languageId": language_id,
                "version": 1,
                "text": content
            }
        })
    
    async def did_change(self, file_path: Path, content: str, version: int = 1):
        """Notify server that a document changed.
        
        Args:
            file_path: Path to the document
            content: Updated content
            version: Document version number
        """
        if not self.initialized:
            raise RuntimeError("TexLab not initialized")
        
        uri = f"file://{file_path.as_posix()}"
        await self._send_notification("textDocument/didChange", {
            "textDocument": {
                "uri": uri,
                "version": version
            },
            "contentChanges": [
                {"text": content}
            ]
        })
    
    async def did_close(self, file_path: Path):
        """Notify server that a document was closed.
        
        Args:
            file_path: Path to the document
        """
        if not self.initialized:
            return
        
        uri = f"file://{file_path.as_posix()}"
        await self._send_notification("textDocument/didClose", {
            "textDocument": {"uri": uri}
        })
    
    async def get_completion(self, file_path: Path, line: int, character: int) -> List[Dict]:
        """Get completion suggestions at position.
        
        Args:
            file_path: Path to the document
            line: Line number (0-based)
            character: Character position (0-based)
            
        Returns:
            List of completion items
        """
        if not self.initialized:
            raise RuntimeError("TexLab not initialized")
        
        uri = f"file://{file_path.as_posix()}"
        result = await self._send_request("textDocument/completion", {
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character}
        })
        
        if result and isinstance(result, dict):
            return result.get("items", [])
        elif isinstance(result, list):
            return result
        
        return []
    
    async def get_hover(self, file_path: Path, line: int, character: int) -> Optional[Dict]:
        """Get hover information at position.
        
        Args:
            file_path: Path to the document
            line: Line number (0-based)
            character: Character position (0-based)
            
        Returns:
            Hover information dict or None
        """
        if not self.initialized:
            raise RuntimeError("TexLab not initialized")
        
        uri = f"file://{file_path.as_posix()}"
        result = await self._send_request("textDocument/hover", {
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character}
        })
        
        return result
    
    async def get_definition(self, file_path: Path, line: int, character: int) -> List[Dict]:
        """Get definition location at position.
        
        Args:
            file_path: Path to the document
            line: Line number (0-based)
            character: Character position (0-based)
            
        Returns:
            List of definition locations
        """
        if not self.initialized:
            raise RuntimeError("TexLab not initialized")
        
        uri = f"file://{file_path.as_posix()}"
        result = await self._send_request("textDocument/definition", {
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character}
        })
        
        if isinstance(result, dict):
            return [result]
        elif isinstance(result, list):
            return result
        
        return []
    
    async def format_document(self, file_path: Path) -> List[Dict]:
        """Format entire document.
        
        Args:
            file_path: Path to the document
            
        Returns:
            List of text edits
        """
        if not self.initialized:
            raise RuntimeError("TexLab not initialized")
        
        uri = f"file://{file_path.as_posix()}"
        result = await self._send_request("textDocument/formatting", {
            "textDocument": {"uri": uri},
            "options": {
                "tabSize": 4,
                "insertSpaces": True
            }
        })
        
        return result or []
    
    async def _send_request(self, method: str, params: Dict) -> Any:
        """Send a request and wait for response.
        
        Args:
            method: LSP method name
            params: Request parameters
            
        Returns:
            Response result
        """
        self.request_id += 1
        request_id = self.request_id
        
        message = LSPMessage(
            id=request_id,
            method=method,
            params=params
        )
        
        # Create future for response
        future = asyncio.Future()
        self.pending_requests[request_id] = future
        
        # Send message
        await self._write_message(message)
        
        # Wait for response
        try:
            result = await asyncio.wait_for(future, timeout=30.0)
            return result
        except asyncio.TimeoutError:
            del self.pending_requests[request_id]
            raise TimeoutError(f"LSP request timed out: {method}")
    
    async def _send_notification(self, method: str, params: Dict):
        """Send a notification (no response expected).
        
        Args:
            method: LSP method name
            params: Notification parameters
        """
        message = LSPMessage(method=method, params=params)
        await self._write_message(message)
    
    async def _write_message(self, message: LSPMessage):
        """Write LSP message to server stdin.
        
        Args:
            message: LSP message to send
        """
        if not self.process or not self.process.stdin:
            raise RuntimeError("TexLab process not running")
        
        # Convert to JSON-RPC format
        content = {}
        if message.id is not None:
            content["id"] = message.id
        if message.method:
            content["method"] = message.method
        if message.params is not None:
            content["params"] = message.params
        if message.result is not None:
            content["result"] = message.result
        if message.error is not None:
            content["error"] = message.error
        
        content["jsonrpc"] = "2.0"
        
        body = json.dumps(content)
        headers = f"Content-Length: {len(body)}\r\n\r\n"
        
        self.process.stdin.write((headers + body).encode('utf-8'))
        await self.process.stdin.drain()
    
    async def _read_messages(self):
        """Read messages from server stdout."""
        if not self.process or not self.process.stdout:
            return
        
        try:
            while True:
                # Read headers
                headers = {}
                while True:
                    line = await self.process.stdout.readline()
                    if not line:
                        return
                    
                    line = line.decode('utf-8').strip()
                    if not line:
                        break
                    
                    key, value = line.split(':', 1)
                    headers[key.strip()] = value.strip()
                
                # Read content
                content_length = int(headers.get('Content-Length', 0))
                if content_length == 0:
                    continue
                
                content = await self.process.stdout.readexactly(content_length)
                message = json.loads(content.decode('utf-8'))
                
                # Handle message
                await self._handle_message(message)
                
        except Exception as e:
            logging.error(f"Error reading LSP messages: {e}")
    
    async def _handle_message(self, message: Dict):
        """Handle incoming LSP message.
        
        Args:
            message: Parsed JSON-RPC message
        """
        # Response to a request
        if "id" in message and message["id"] in self.pending_requests:
            future = self.pending_requests.pop(message["id"])
            
            if "error" in message:
                future.set_exception(Exception(message["error"].get("message", "Unknown error")))
            else:
                future.set_result(message.get("result"))
        
        # Notification from server
        elif "method" in message:
            method = message["method"]
            params = message.get("params", {})
            
            if method == "textDocument/publishDiagnostics":
                await self._handle_diagnostics(params)
    
    async def _handle_diagnostics(self, params: Dict):
        """Handle diagnostic notifications.
        
        Args:
            params: Diagnostics notification parameters
        """
        uri = params.get("uri", "")
        diagnostics_data = params.get("diagnostics", [])
        
        diagnostics = [
            Diagnostic(
                range=d["range"],
                severity=d.get("severity", DiagnosticSeverity.ERROR.value),
                message=d.get("message", ""),
                source=d.get("source", "texlab"),
                code=d.get("code")
            )
            for d in diagnostics_data
        ]
        
        self.diagnostics[uri] = diagnostics
        
        # Call callback if registered
        if self.on_diagnostics:
            self.on_diagnostics(uri, diagnostics)
        
        logging.debug(f"Received {len(diagnostics)} diagnostics for {uri}")
