"""Security utilities for file access, path validation, and input sanitization."""

import os
import re
from pathlib import Path
from typing import Optional


class SecurityError(Exception):
    """Raised when a security check fails."""
    pass


# Allowed file extensions for editing
ALLOWED_EXTENSIONS = {'.tex', '.bib', '.sty', '.cls', '.tikz'}

# Reject executable and sensitive extensions
BLOCKED_EXTENSIONS = {
    '.py', '.sh', '.bat', '.exe', '.dll', '.so', '.o',
    '.zip', '.tar', '.gz', '.rar', '.7z',
    '.env', '.key', '.pem', '.cert'
}

# Prompt injection patterns
PROMPT_INJECTION_PATTERNS = [
    r'ignore\s+previous',
    r'system\s*:',
    r'<\|im_start\|>',
    r'###\s*Instruction',
]


def sanitize_user_instruction(instruction: str, max_length: int = 5000) -> str:
    """Sanitize user instruction to prevent prompt injection.
    
    Args:
        instruction: User-provided instruction
        max_length: Maximum allowed length (default 5000)
        
    Raises:
        ValueError: If instruction fails safety checks
    """
    import html
    import logging
    
    if not instruction or not isinstance(instruction, str):
        raise ValueError("Instruction must be non-empty string")
    
    if len(instruction) > max_length:
        raise ValueError(f"Instruction exceeds {max_length} chars")
    
    # Remove control characters
    instruction = re.sub(r'[\x00-\x08\x0B-\x0C\x0E-\x1F\x7F]', '', instruction)
    
    # Check for injection patterns
    for pattern in PROMPT_INJECTION_PATTERNS:
        if re.search(pattern, instruction, re.IGNORECASE):
            logging.warning(f"Blocked injection attempt: {instruction[:100]}")
            raise ValueError("Instruction contains suspicious content")
    
    # HTML escape for log safety
    return html.escape(instruction.strip())



def validate_workspace_path(workspace_path: str) -> Path:
    """Validate and resolve workspace root path.
    
    Args:
        workspace_path: Path to workspace root
        
    Returns:
        Resolved absolute Path
        
    Raises:
        SecurityError: If path is invalid or inaccessible
    """
    try:
        path = Path(workspace_path).resolve()
        
        if not path.exists():
            raise SecurityError(f"Workspace path does not exist: {path}")
        
        if not path.is_dir():
            raise SecurityError(f"Workspace path is not a directory: {path}")
        
        # Check permissions
        if not os.access(path, os.R_OK | os.W_OK | os.X_OK):
            raise SecurityError(f"Insufficient permissions for workspace: {path}")
        
        return path
    except SecurityError:
        raise
    except Exception as e:
        raise SecurityError(f"Invalid workspace path: {e}")


def safe_resolve_path(workspace_root: Path, requested_path: str) -> Path:
    """Safely resolve a requested path within workspace, blocking symlink escapes.
    
    Args:
        workspace_root: Validated workspace root path
        requested_path: User-requested relative path
        
    Returns:
        Resolved absolute path (guaranteed to be within workspace_root)
        
    Raises:
        SecurityError: If path escape attempt detected or contains invalid elements
    """
    # Reject null bytes
    if '\x00' in requested_path:
        raise SecurityError("Null bytes in path")

    # Normalize workspace root first
    workspace_root = workspace_root.resolve()

    # Resolve absolute paths directly, otherwise join to workspace
    if os.path.isabs(requested_path):
        target = Path(requested_path)
    else:
        # Reject suspicious patterns
        if '..' in requested_path:
            raise SecurityError("Path traversal (..) not allowed: outside workspace")

        # Only strip explicit './' prefixes; preserve leading dots so hidden files are detected
        while requested_path.startswith('./'):
            requested_path = requested_path[2:]

        target = workspace_root / requested_path
    
    # Check that unresolved path is within workspace BEFORE resolving
    try:
        target_abs = target.absolute()
        target_abs.relative_to(workspace_root)
    except ValueError:
        raise SecurityError(f"Path is outside workspace: {requested_path}")
    
    # Check symlink safety BEFORE resolving
    if target.exists() and target.is_symlink():
        # Resolve symlink and verify it stays in workspace
        real_target = target.resolve()
        try:
            real_target.relative_to(workspace_root)
        except ValueError:
            raise SecurityError(f"Symlink points outside workspace: {requested_path} -> {real_target}")
    
    # Now safe to resolve
    resolved = target.resolve()
    
    # Double-check containment after resolution
    try:
        resolved.relative_to(workspace_root)
    except ValueError:
        raise SecurityError(f"Path is outside workspace: {resolved}")

    # Block hidden files/directories (dot-prefixed) inside workspace
    for part in resolved.relative_to(workspace_root).parts:
        if part.startswith('.'):
            raise SecurityError(f"Hidden files are not allowed: {resolved}")
    
    return resolved


def check_symlink_safety(path: Path, workspace_root: Path) -> None:
    """Check if a symlink escapes the workspace.
    
    Args:
        path: Path to check (may be a symlink)
        workspace_root: Workspace root for containment check
        
    Raises:
        SecurityError: If symlink points outside workspace or is circular
    """
    # Reject symlinks to parent directories
    if path.is_symlink():
        target = path.resolve()
        
        try:
            target.relative_to(workspace_root)
        except ValueError:
            raise SecurityError(f"Symlink escapes workspace: {path} -> {target}")
    
    # Check all parent directories for symlinks
    current = path.parent
    while current != current.parent:  # Stop at filesystem root
        if current.is_symlink():
            target = current.resolve()
            try:
                target.relative_to(workspace_root)
            except ValueError:
                raise SecurityError(f"Parent symlink escapes workspace: {current} -> {target}")
        
        try:
            current.relative_to(workspace_root)
        except ValueError:
            break  # Reached outside workspace
        
        current = current.parent


def check_file_type(path: Path, allow_read_only: bool = False) -> None:
    """Check if file type is allowed for editing.
    
    Args:
        path: Path to file
        allow_read_only: If True, allows read of non-editable files
        
    Raises:
        SecurityError: If file type is not allowed
    """
    extension = path.suffix.lower()
    
    # Block dangerous extensions
    if extension in BLOCKED_EXTENSIONS:
        raise SecurityError(f"File type not allowed: {extension}")
    
    # For edit operations, restrict to whitelist
    if not allow_read_only and extension not in ALLOWED_EXTENSIONS:
        allowed = ', '.join(sorted(ALLOWED_EXTENSIONS))
        raise SecurityError(f"Can only edit: {allowed}. Got: {extension}")


def validate_file_access(
    workspace_root: Path,
    requested_path: str,
    access_type: str = "read"
) -> Path:
    """Comprehensive file access validation.
    
    Args:
        workspace_root: Validated workspace root
        requested_path: User-requested file path
        access_type: "read", "write", or "delete"
        
    Returns:
        Validated absolute path
        
    Raises:
        SecurityError: If any check fails
    """
    # Resolve path safely
    resolved_path = safe_resolve_path(workspace_root, requested_path)
    
    # Check symlinks
    check_symlink_safety(resolved_path, workspace_root)
    
    # Check file type
    allow_read_only = (access_type == "read")
    check_file_type(resolved_path, allow_read_only=allow_read_only)
    
    # Check file exists (for read only). Writes may create new files.
    if access_type == "read" and not resolved_path.exists():
        raise SecurityError(f"File not found: {resolved_path}")
    
    # Check permissions
    if access_type == "read" and not os.access(resolved_path, os.R_OK):
        raise SecurityError(f"Cannot read file (permission denied): {resolved_path}")
    
    if access_type == "write" and resolved_path.exists() and not os.access(resolved_path, os.W_OK):
        raise SecurityError(f"Cannot write file (permission denied): {resolved_path}")
    
    return resolved_path
