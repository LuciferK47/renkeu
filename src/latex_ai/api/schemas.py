"""Pydantic schemas used across API routers."""
from __future__ import annotations

from typing import Any, Dict, List, Optional

from pydantic import BaseModel, Field, field_validator

from latex_ai.models_registry import ModelInfo


class ApplyRequest(BaseModel):
    file_path: str = Field(..., description="Absolute or workspace-relative path to the file")
    command: str = Field(..., min_length=1, description="LaTeX command to apply")
    dry_run: bool = Field(False, description="If true, returns preview without making changes")
    if_match: Optional[str] = Field(None, description="ETag for optimistic concurrency control")
    
    @field_validator('file_path')
    @classmethod
    def validate_file_path(cls, v: str) -> str:
        if not v or not v.strip():
            raise ValueError("file_path cannot be empty")
        return v.strip()
    
    @field_validator('command')
    @classmethod
    def validate_command(cls, v: str) -> str:
        if not v or not v.strip():
            raise ValueError("command cannot be empty")
        if len(v) > 10000:
            raise ValueError("command exceeds maximum length of 10000 characters")
        return v.strip()


class ApplyResponse(BaseModel):
    success: bool
    message: str
    changed: bool
    diff: Optional[str] = None
    backup_path: Optional[str] = None
    history_id: Optional[int] = None


class PreviewResponse(BaseModel):
    file_path: str
    documentclass: str
    geometry: str
    packages: List[str] = Field(default_factory=list)


class PlanRequest(BaseModel):
    file_path: str = Field(..., description="Path to the LaTeX file to analyze")
    request_text: str = Field(..., min_length=1, max_length=5000, description="Natural language editing request")
    model: Optional[str] = Field(None, description="AI model to use (defaults to configured model)")
    
    @field_validator('file_path')
    @classmethod
    def validate_file_path(cls, v: str) -> str:
        if not v or not v.strip():
            raise ValueError("file_path cannot be empty")
        return v.strip()
    
    @field_validator('request_text')
    @classmethod
    def validate_request_text(cls, v: str) -> str:
        if not v or not v.strip():
            raise ValueError("request_text cannot be empty")
        return v.strip()


class ModelsResponse(BaseModel):
    current: str
    available: List[ModelInfo]


class HistoryResponse(BaseModel):
    id: int
    command: str
    timestamp: str
    diff: str


class HistoryEntryResponse(BaseModel):
    """Detailed history entry for batch responses."""
    id: int
    file_path: str
    command: str
    timestamp: str
    diff: str
    success: bool = True


class PaginatedHistoryResponse(BaseModel):
    """Paginated history response with cursor support."""
    items: List[HistoryResponse]
    limit: int
    offset: int
    total: int
    has_more: bool  # True if more entries exist beyond current page


class FileResponseModel(BaseModel):
    content: str


class CompileRequest(BaseModel):
    file_path: str = Field(..., description="Path to main LaTeX file to compile")
    main_file: Optional[str] = Field(None, description="Optional override for main file")
    output_dir: Optional[str] = Field(None, description="Output directory for PDF")
    
    @field_validator('file_path')
    @classmethod
    def validate_file_path(cls, v: str) -> str:
        if not v or not v.strip():
            raise ValueError("file_path cannot be empty")
        if not v.endswith('.tex'):
            raise ValueError("file_path must point to a .tex file")
        return v.strip()


class CompileStatusResponse(BaseModel):
    job_id: str
    status: str
    progress: float
    pdf_url: Optional[str] = None
    diagnostics: Dict[str, Any]
    error: Optional[str] = None
    duration: Optional[float] = None


class CompileResponse(BaseModel):
    success: bool
    pdf_path: Optional[str] = None
    pdf_url: Optional[str] = None
    synctex_path: Optional[str] = None
    page_count: int = 0
    compile_time: float = 0.0
    errors: List[Dict[str, Any]] = Field(default_factory=list)
    warnings: List[Dict[str, Any]] = Field(default_factory=list)


class SaveFileRequest(BaseModel):
    path: str = Field(..., description="Absolute or workspace-relative file path")
    content: str = Field(..., description="File content to save")

    @field_validator("path")
    @classmethod
    def validate_path(cls, value: str) -> str:
        if not value or not value.strip():
            raise ValueError("path cannot be empty")
        return value.strip()

    @field_validator("content")
    @classmethod
    def validate_content_size(cls, value: str) -> str:
        byte_size = len(value.encode("utf-8"))
        if byte_size > 10 * 1024 * 1024:
            size_mb = byte_size / (1024 * 1024)
            raise ValueError(f"File too large: {size_mb:.1f}MB exceeds 10MB limit")
        return value


class RestoreRequest(BaseModel):
    history_id: int = Field(..., gt=0, description="ID of history entry to restore")
    file_path: str = Field(..., description="Path to file being restored")
    
    @field_validator('file_path')
    @classmethod
    def validate_file_path(cls, v: str) -> str:
        if not v or not v.strip():
            raise ValueError("file_path cannot be empty")
        return v.strip()


class ConfigResponse(BaseModel):
    keyboard_shortcuts: dict = Field(default_factory=dict)
    theme: dict = Field(default_factory=dict)
    editor: dict = Field(default_factory=dict)
    auto_save: bool = True
    history_limit: int = 100
    ai_model_default: Optional[str] = None
    available_models: Optional[list] = None


class ListFilesResponse(BaseModel):
    files: List[dict] = Field(default_factory=list)
    total: Optional[int] = None
    offset: Optional[int] = None
    limit: Optional[int] = None
    has_more: Optional[bool] = None


class BulkApplyRequest(BaseModel):
    files: List[str]
    command: str
    etags: Optional[Dict[str, str]] = Field(default=None, description="Optional ETags per file for conflict detection")


class BulkApplyResponse(BaseModel):
    results: List[dict] = Field(default_factory=list)
    conflicts: Optional[List[dict]] = Field(default=None, description="Files with ETag conflicts")


class SecurityInfoResponse(BaseModel):
    allowed_extensions: List[str]
    blocked_extensions: List[str]
    max_file_size_mb: Optional[int] = None
    path_traversal_protection: bool = True
    symlink_detection: bool = True


class SyncTeXForwardRequest(BaseModel):
    file_path: str
    line: int
    synctex_file: Optional[str] = None


class SyncTeXBackwardRequest(BaseModel):
    synctex_file: str
    page: int
    x: float
    y: float


class SyncTeXResponse(BaseModel):
    success: bool
    file: Optional[str] = None
    line: Optional[int] = None
    column: Optional[int] = None
    page: Optional[int] = None
    x: Optional[float] = None
    y: Optional[float] = None
    error: Optional[str] = None
    max_file_size_mb: int | None = None
    path_traversal_protection: bool = True
    symlink_detection: bool = True


class AgentEdit(BaseModel):
    """Agent edit operation with 1-based line numbering.
    
    Line numbers follow human-readable conventions:
    - First line of file is line 1 (not 0)
    - insert: line=5 inserts BEFORE line 5
    - replace_range: start=2, end=4 replaces lines 2,3,4 (inclusive)
    """
    file_path: str
    op: str
    payload: str = ""
    expected_etag: Optional[str] = None
    start_line: Optional[int] = Field(None, ge=1, description="1-based start line (inclusive)")
    end_line: Optional[int] = Field(None, ge=1, description="1-based end line (inclusive)")
    text: Optional[str] = None
    line: Optional[int] = Field(None, ge=1, description="1-based line number")
    insert_text: Optional[str] = None


class AgentRequest(BaseModel):
    edits: List[AgentEdit]
    auto_compile: bool = False
    compile_timeout: int = 60


class AgentEditResult(BaseModel):
    file_path: str
    success: bool
    message: str
    diff: Optional[str] = None
    conflict: Optional[dict] = None


class AgentResponse(BaseModel):
    success: bool
    results: List[AgentEditResult] = Field(default_factory=list)
    compile_result: Optional[dict] = None


class AgentLoopRequest(BaseModel):
    file_path: str
    instruction: str
    model: Optional[str] = None
    max_iterations: int = 3
    auto_compile: bool = True
    auto_apply: bool = True


class AgentLoopEvent(BaseModel):
    type: str
    iteration: int
    data: dict
