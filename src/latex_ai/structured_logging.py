"""Structured logging setup for LaTeX-AI server."""

import logging
import time
from typing import Any, Callable, Optional
from functools import wraps

try:
    import structlog
    HAS_STRUCTLOG = True
except ImportError:
    HAS_STRUCTLOG = False
    structlog = None


def setup_structured_logging(
    log_level: str = "INFO",
    json_output: bool = False
) -> None:
    """Configure structured logging with structlog (or fallback to standard logging).
    
    Args:
        log_level: Logging level (DEBUG, INFO, WARNING, ERROR, CRITICAL)
        json_output: If True, output JSON format (for production)
    """
    
    # Configure standard logging to prevent duplicate logs
    logging.basicConfig(
        format="%(asctime)s - %(name)s - %(levelname)s - %(message)s",
        level=log_level,
        handlers=[logging.StreamHandler()],
    )
    
    # Configure structlog if available
    if HAS_STRUCTLOG:
        if json_output:
            # Production: JSON format
            processors = [
                structlog.stdlib.add_log_level,
                structlog.processors.TimeStamper(fmt="iso"),
                structlog.processors.JSONRenderer(),
            ]
        else:
            # Development: Human-readable
            processors = [
                structlog.stdlib.add_log_level,
                structlog.processors.TimeStamper(fmt="iso"),
                structlog.dev.ConsoleRenderer(),
            ]
        
        structlog.configure(
            processors=processors,
            context_class=dict,
            logger_factory=structlog.stdlib.LoggerFactory(),
            cache_logger_on_first_use=True,
        )


def get_logger(name: str):
    """Get a bound logger with module name context.
    
    Args:
        name: Logger name (usually __name__)
        
    Returns:
        Configured structlog logger or standard logging fallback
    """
    if HAS_STRUCTLOG:
        return structlog.get_logger(name)
    else:
        # Fallback to standard logging
        return logging.getLogger(name)


def log_request_summary(func: Callable) -> Callable:
    """Decorator to log HTTP request/response with timing.
    
    Logs:
    - request_method, request_path, request_user
    - response_status, response_time_ms
    - error details if exception raised
    """
    @wraps(func)
    async def wrapper(*args, **kwargs) -> Any:
        # Extract request if available (depends on function signature)
        request = None
        for arg in args:
            if hasattr(arg, 'method') and hasattr(arg, 'url'):
                request = arg
                break
        
        logger = get_logger("request")
        method = request.method if request else "UNKNOWN"
        path = str(request.url.path) if request else "UNKNOWN"
        
        start_time = time.time()
        status = 500
        
        try:
            result = await func(*args, **kwargs)
            
            # Try to extract status code
            if hasattr(result, 'status_code'):
                status = result.status_code
            else:
                status = 200
            
            return result
        except Exception as e:
            status = 500
            logger.exception(
                "request_error",
                method=method,
                path=path,
                error=str(e),
                duration_ms=round((time.time() - start_time) * 1000)
            )
            raise
        finally:
            elapsed_ms = round((time.time() - start_time) * 1000)
            logger.info(
                "request_completed",
                method=method,
                path=path,
                status=status,
                duration_ms=elapsed_ms
            )
    
    return wrapper


def log_agent_request(
    instruction: str,
    file_path: str,
    status: str = "started",
    duration_ms: Optional[int] = None,
    error: Optional[str] = None,
    proposal_count: Optional[int] = None
) -> None:
    """Log agent request with structured context.
    
    Args:
        instruction: User instruction
        file_path: Target file being modified
        status: "started", "completed", "failed"
        duration_ms: Time spent (for completed/failed)
        error: Error message if failed
        proposal_count: Number of proposals generated
    """
    logger = get_logger("agent")
    
    log_data = {
        "status": status,
        "instruction": instruction[:100],  # Truncate for security
        "file_path": file_path,
    }
    
    if duration_ms is not None:
        log_data["duration_ms"] = duration_ms
    
    if error:
        log_data["error"] = error
    
    if proposal_count is not None:
        log_data["proposal_count"] = proposal_count
    
    if status == "failed":
        logger.error("agent_request_failed", **log_data)
    elif status == "completed":
        logger.info("agent_request_completed", **log_data)
    else:
        logger.info("agent_request_started", **log_data)


def log_file_operation(
    operation: str,
    file_path: str,
    success: bool,
    duration_ms: Optional[int] = None,
    error: Optional[str] = None,
    bytes_changed: Optional[int] = None
) -> None:
    """Log file operation with details.
    
    Args:
        operation: "read", "write", "delete", "compile"
        file_path: Target file
        success: Whether operation succeeded
        duration_ms: Time spent
        error: Error if failed
        bytes_changed: Number of bytes changed (for writes)
    """
    logger = get_logger("file_operation")
    
    log_data = {
        "operation": operation,
        "file_path": file_path,
        "success": success,
    }
    
    if duration_ms is not None:
        log_data["duration_ms"] = duration_ms
    
    if error:
        log_data["error"] = error
    
    if bytes_changed is not None:
        log_data["bytes_changed"] = bytes_changed
    
    if success:
        logger.info("file_operation_success", **log_data)
    else:
        logger.warning("file_operation_failed", **log_data)


def log_compilation(
    file_path: str,
    success: bool,
    duration_ms: int,
    error_count: int = 0,
    warning_count: int = 0
) -> None:
    """Log LaTeX compilation result.
    
    Args:
        file_path: Compiled file
        success: Compilation succeeded
        duration_ms: Time spent
        error_count: Number of compilation errors
        warning_count: Number of warnings
    """
    logger = get_logger("compile")
    
    log_data = {
        "file_path": file_path,
        "success": success,
        "duration_ms": duration_ms,
        "error_count": error_count,
        "warning_count": warning_count,
    }
    
    if success:
        logger.info("compilation_success", **log_data)
    else:
        logger.warning("compilation_failed", **log_data)
