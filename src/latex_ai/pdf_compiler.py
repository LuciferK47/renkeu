"""PDF compilation engine for LaTeX documents."""

import subprocess
import re
import tempfile
import shutil
import signal
import os
import hashlib
import json
from pathlib import Path
from typing import Dict, List, Optional, Tuple, Set
from dataclasses import dataclass, field
from datetime import datetime

# Limits to prevent excessive copying during compilation
MAX_COPY_FILES = 2000
MAX_COPY_BYTES = 50 * 1024 * 1024  # 50 MB
DEFAULT_IGNORE_DIRS = {
    '.git', 'node_modules', '.venv', 'venv', 'dist', 'build',
    '__pycache__', '.pytest_cache', '.mypy_cache', '.idea'
}

# Resource limits for compilation processes
MAX_COMPILE_TIME = 120  # 120 seconds max
MAX_COMPILE_MEMORY = 512 * 1024 * 1024  # 512MB
MAX_COMPILE_CPU_PERCENT = 90  # Max 90% CPU usage

# Dangerous LaTeX commands that enable arbitrary shell execution
DANGEROUS_COMMANDS = {
    r'\write18',      # Shell escape command
    r'\input|',       # Piped input
    r'\openin',       # Open input stream
    r'\openout',      # Open output stream
}


@dataclass
class CompileError:
    """Represents a LaTeX compilation error."""
    
    line: int
    message: str
    file: str = ""
    error_type: str = "error"  # error, warning, undefined_reference
    
    def __str__(self) -> str:
        return f"Line {self.line}: {self.message}"


@dataclass
class CompileResult:
    """Result of PDF compilation."""
    
    success: bool
    pdf_path: Optional[str] = None
    errors: List[CompileError] = field(default_factory=list)
    warnings: List[CompileError] = field(default_factory=list)
    log: str = ""
    compile_time: float = 0.0
    page_count: int = 0
    synctex_path: Optional[str] = None  # Path to .synctex.gz file for PDF ↔ source mapping


class PDFCompiler:
    """Compiles LaTeX documents to PDF using pdflatex."""
    
    def __init__(self, working_dir: Optional[str] = None):
        """
        Initialize PDF compiler.
        
        Args:
            working_dir: Directory for temporary files (default: system temp)
        """
        self.working_dir = Path(working_dir) if working_dir else Path(tempfile.gettempdir())
        self.working_dir.mkdir(parents=True, exist_ok=True)
        self._processes: Dict[str, subprocess.Popen] = {}  # job_id -> process for concurrent tracking
        self._cache_dir = self.working_dir / ".compile_cache"
        self._cache_dir.mkdir(parents=True, exist_ok=True)
        self._content_hashes: Dict[str, str] = {}  # In-memory cache: content_hash -> pdf_path
        self._max_cache_entries: int = int(os.getenv("MAX_COMPILE_CACHE_ENTRIES", "20"))
    
    def _validate_filename(self, file_path: str) -> Optional[str]:
        """
        Validate filename for injection attacks.
        
        Checks for dangerous patterns in filenames that could be exploited:
        - Shell metacharacters (|, ;, &, $, `, etc.)
        - Path traversal sequences (.., ~, etc.)
        - Suspicious quotes or escapes
        
        Args:
            file_path: File path to validate
            
        Returns:
            Error message if invalid, None if valid
        """
        filename = Path(file_path).name
        
        # Check for shell metacharacters in filename
        dangerous_chars = ['|', ';', '&', '$', '`', '\n', '\r']
        for char in dangerous_chars:
            if char in filename:
                return f"Filename contains dangerous character '{char}': {filename}"
        
        # Check for suspicious patterns
        if '..' in filename or '~' in filename:
            return f"Filename contains path traversal pattern: {filename}"
        
        # Check for excessive quoting or escaping
        if filename.count('"') > 2 or filename.count("'") > 2 or filename.count('\\') > 1:
            return f"Filename contains suspicious quoting or escaping: {filename}"
        
        return None
    
    def _validate_latex_content(self, content: str) -> List[str]:
        """
        Validate LaTeX content for dangerous commands and injection attacks.
        
        Checks for:
        - Shell escape commands (\write18, \input|, etc.)
        - Comment injection attempts
        - Shell metacharacters in content
        
        Args:
            content: LaTeX document content
            
        Returns:
            List of error messages (empty if safe)
        """
        errors = []
        lines = content.split('\n')
        
        # Dangerous shell metacharacters that could be exploited
        shell_chars_pattern = re.compile(r'[;&|`$\(\)<>]')
        
        for line_num, line in enumerate(lines, start=1):
            # Skip fully commented lines
            if line.strip().startswith('%'):
                continue
            
            # Check for dangerous LaTeX commands
            for cmd in DANGEROUS_COMMANDS:
                if cmd in line:
                    errors.append(
                        f"Line {line_num}: Dangerous command '{cmd}' detected. "
                        f"This command can execute arbitrary shell commands and is blocked for security."
                    )
            
            # Check for comment injection (unusual use of % in command arguments)
            # Look for patterns like \command{...%...} which could hide commands in comments
            if re.search(r'\\[a-zA-Z]+\{[^}]*%[^}]*\}', line):
                errors.append(
                    f"Line {line_num}: Potential comment injection in command. "
                    f"Comments inside command arguments are suspicious."
                )
            
            # Check for shell metacharacters in potentially dangerous contexts
            # (only check in certain patterns to avoid false positives with math)
            if '\\' in line and shell_chars_pattern.search(line):
                # Check specifically for shell commands in backslash sequences
                if re.search(r'\\[a-zA-Z]*\s*[;&|`$]', line):
                    errors.append(
                        f"Line {line_num}: Suspicious shell metacharacter in command context. "
                        f"Shell characters like &, |, ;, `, $ may enable code injection."
                    )
        
        return errors
    
    def _compute_content_hash(self, content: str, config: Optional[Dict] = None) -> str:
        """
        Compute semantic hash of LaTeX content and compile configuration.
        
        Args:
            content: LaTeX document content
            config: Optional compile config (max_iterations, etc.)
            
        Returns:
            SHA256 hash of content + config
        """
        hasher = hashlib.sha256()
        hasher.update(content.encode('utf-8'))
        
        if config:
            config_str = json.dumps(config, sort_keys=True)
            hasher.update(config_str.encode('utf-8'))
        
        return hasher.hexdigest()
    
    async def _compute_bundle_hash(self, main_file: Path, config: Optional[Dict] = None) -> str:
        """
        Compute hash of entire bundle (main file + all dependencies).
        
        This ensures cache invalidation when ANY file in the project changes,
        not just the main file.
        
        Args:
            main_file: Path to main LaTeX file
            config: Optional compile config (max_iterations, etc.)
            
        Returns:
            SHA256 hash of bundle + config
        """
        from .document_bundle import DocumentBundle
        
        hasher = hashlib.sha256()
        
        # Load bundle with all dependencies
        bundle = DocumentBundle(main_file, workspace_root=main_file.parent)
        await bundle.load_async()
        
        # Hash main file content
        main_content = bundle.files.get(str(main_file), "")
        hasher.update(f"MAIN:{main_file.name}:".encode('utf-8'))
        hasher.update(main_content.encode('utf-8'))
        
        # Hash all dependencies in sorted order (stable ordering)
        # Use full relative path to avoid collisions between files with same basename
        for dep_path in sorted(bundle.files.keys()):
            if dep_path != str(main_file):
                content = bundle.files[dep_path]
                # Use workspace-relative path to avoid collisions (e.g., sections/intro.tex vs appendix/intro.tex)
                try:
                    dep_rel = str(Path(dep_path).relative_to(main_file.parent).as_posix())
                except ValueError:
                    # If not relative to main file parent, use absolute posix path
                    dep_rel = str(Path(dep_path).as_posix())
                hasher.update(f"\nDEP:{dep_rel}:".encode('utf-8'))
                hasher.update(content.encode('utf-8'))
        
        # Include config in hash
        if config:
            config_str = json.dumps(config, sort_keys=True)
            hasher.update(f"\nCONFIG:".encode('utf-8'))
            hasher.update(config_str.encode('utf-8'))
        
        return hasher.hexdigest()
    
    def _get_cached_pdf(self, content_hash: str) -> Optional[Path]:
        """
        Retrieve cached PDF path if it exists and is valid.
        
        Args:
            content_hash: SHA256 hash of content
            
        Returns:
            Path to cached PDF if valid, else None
        """
        cache_file = self._cache_dir / f"{content_hash}.json"
        
        if not cache_file.exists():
            return None
        
        try:
            with open(cache_file, 'r') as f:
                cache_meta = json.load(f)
            
            pdf_path = Path(cache_meta['pdf_path'])
            if pdf_path.exists():
                # Touch metadata to reflect recent access (LRU)
                try:
                    cache_meta['timestamp'] = datetime.now().isoformat()
                    with open(cache_file, 'w') as wf:
                        json.dump(cache_meta, wf)
                except Exception:
                    pass
                return pdf_path
        except (json.JSONDecodeError, KeyError, OSError):
            pass
        
        return None
    
    def _save_cache_entry(self, content_hash: str, pdf_path: Path) -> None:
        """
        Save compilation cache entry.
        
        Args:
            content_hash: SHA256 hash of content
            pdf_path: Path to compiled PDF
        """
        try:
            cache_file = self._cache_dir / f"{content_hash}.json"
            cache_meta = {
                'pdf_path': str(pdf_path),
                'timestamp': datetime.now().isoformat(),
                'exists': pdf_path.exists()
            }
            
            with open(cache_file, 'w') as f:
                json.dump(cache_meta, f)
            # Enforce LRU cache size limit
            self._enforce_cache_limit()
        except (OSError, TypeError):
            # Cache write failures should not block compilation
            pass

    def _enforce_cache_limit(self) -> None:
        """Ensure compile cache does not exceed max entries by evicting oldest.
        Evicts both metadata and corresponding PDF files if present.
        """
        try:
            meta_files = sorted(
                [p for p in self._cache_dir.glob('*.json') if p.is_file()],
                key=lambda p: p.stat().st_mtime
            )
            if len(meta_files) <= self._max_cache_entries:
                return
            to_evict = meta_files[: max(0, len(meta_files) - self._max_cache_entries)]
            for meta in to_evict:
                try:
                    with open(meta, 'r') as f:
                        cache_meta = json.load(f)
                    pdf_path = Path(cache_meta.get('pdf_path', ''))
                except Exception:
                    pdf_path = None
                # Remove meta file
                try:
                    meta.unlink(missing_ok=True)
                except Exception:
                    pass
                # Remove cached PDF if located under cache dir and exists
                try:
                    if pdf_path and pdf_path.exists() and pdf_path.parent == self._cache_dir:
                        pdf_path.unlink(missing_ok=True)
                except Exception:
                    pass
        except Exception:
            # Best-effort eviction; ignore failures
            pass
    
    def _prepare_temp_dir(self, main_file: Path, temp_dir: Path) -> Path:
        """Copy/symlink project structure to temp directory for multi-file compilation.
        
        Uses symlinks for large binary assets (images, PDFs) and copies only text files
        to drastically improve performance for large projects.
        
        Args:
            main_file: Main LaTeX file being compiled
            temp_dir: Temporary directory for compilation
            
        Returns:
            Path to main file in temp directory
        """
        project_root = main_file.parent
        
        # Text files that must be copied (LaTeX sources that will be modified)
        copy_extensions = {'.tex', '.bib', '.cls', '.sty', '.bst', '.bbx', '.cbx', '.def', '.cfg'}
        
        # Binary assets that can be symlinked (images, PDFs, etc.)
        symlink_extensions = {'.png', '.jpg', '.jpeg', '.pdf', '.eps', '.svg', '.gif', '.tif', '.tiff'}
        
        # Combine all relevant extensions
        all_extensions = copy_extensions | symlink_extensions
        
        # Process relevant files maintaining directory structure
        for item in project_root.rglob("*"):
            if not item.is_file() or item.suffix.lower() not in all_extensions:
                continue
                
            try:
                rel_path = item.relative_to(project_root)
                dest = temp_dir / rel_path
                
                # Create parent directories
                dest.parent.mkdir(parents=True, exist_ok=True)
                
                # Use symlinks for large binary assets (much faster)
                if item.suffix.lower() in symlink_extensions:
                    # Symlink is almost instantaneous regardless of file size
                    try:
                        dest.symlink_to(item)
                    except (OSError, NotImplementedError):
                        # Fallback to copy if symlinks not supported (rare on modern systems)
                        shutil.copy2(item, dest)
                else:
                    # Copy text files (needed for potential modifications)
                    shutil.copy2(item, dest)
                    
            except (ValueError, OSError):
                # Skip files that can't be processed
                continue
        
        # Return path to main file in temp directory
        return temp_dir / main_file.name
    
    def _verify_pdflatex(self) -> None:
        """Verify pdflatex is available on system."""
        try:
            result = subprocess.run(
                ["pdflatex", "-version"],
                capture_output=True,
                timeout=5
            )
            if result.returncode != 0:
                raise RuntimeError("pdflatex not found or not working")
        except FileNotFoundError:
            raise RuntimeError(
                "pdflatex not found. Install it with:\n"
                "  Ubuntu/Debian: sudo apt-get install texlive-latex-base\n"
                "  macOS: brew install basictex\n"
                "  Windows: Install MiKTeX (https://miktex.org/download)"
            )
    
    def compile(
        self,
        tex_content: str,
        output_dir: Optional[str] = None,
        max_iterations: int = 3
    ) -> CompileResult:
        """
        Compile LaTeX content to PDF with semantic hash caching and input validation.
        
        Args:
            tex_content: LaTeX document content
            output_dir: Directory for output PDF (default: temp dir)
            max_iterations: Max compile passes (for cross-refs, TOC, etc.)
        
        Returns:
            CompileResult with success status, PDF path, and errors
        """
        import time
        start_time = time.time()
        
        # Validate output directory filename if provided
        if output_dir:
            filename_error = self._validate_filename(output_dir)
            if filename_error:
                return CompileResult(
                    success=False,
                    errors=[CompileError(0, filename_error)]
                )
        
        # Validate LaTeX content for dangerous commands and injection attacks
        validation_errors = self._validate_latex_content(tex_content)
        if validation_errors:
            return CompileResult(
                success=False,
                errors=[CompileError(0, err) for err in validation_errors]
            )
        
        # Check cache before compiling
        config = {'max_iterations': max_iterations}
        content_hash = self._compute_content_hash(tex_content, config)
        
        cached_pdf = self._get_cached_pdf(content_hash)
        if cached_pdf:
            try:
                compile_time = time.time() - start_time
                # Return cached PDF without recompilation
                return CompileResult(
                    success=True,
                    pdf_path=str(cached_pdf),
                    compile_time=compile_time
                )
            except Exception:
                pass  # Fall through to regular compilation if cache read fails
        
        # Use a per-run temp directory when caller does not specify output_dir to avoid buildup
        temp_dir_obj = None
        try:
            if output_dir is None:
                temp_dir_obj = tempfile.TemporaryDirectory(prefix="latex_ai_", dir=self.working_dir)
                output_dir_path = Path(temp_dir_obj.name)
            else:
                output_dir_path = Path(output_dir)
                output_dir_path.mkdir(parents=True, exist_ok=True)
            
            # Create temporary tex file
            tex_file = output_dir_path / "document.tex"
            try:
                tex_file.write_text(tex_content, encoding='utf-8')
            except Exception as e:
                return CompileResult(
                    success=False,
                    errors=[CompileError(0, f"Failed to write tex file: {e}")]
                )
            
            pdf_path = output_dir_path / "document.pdf"
            aux_file = output_dir_path / "document.aux"
            log_file = output_dir_path / "document.log"
            
            errors = []
            warnings = []
            
            # Run pdflatex multiple times
            for iteration in range(max_iterations):
                result = self._run_pdflatex(tex_file, output_dir_path)
                
                if result['returncode'] != 0 and iteration == max_iterations - 1:
                    # Parse log for errors
                    log_content = self._read_log(log_file)
                    errors, warnings = self._parse_log(log_content)
                    
                    compile_time = time.time() - start_time
                    result = CompileResult(
                        success=False,
                        errors=errors,
                        warnings=warnings,
                        log=log_content,
                        compile_time=compile_time
                    )
                    return result
                
                # Check if PDF was generated
                if not pdf_path.exists() and iteration == max_iterations - 1:
                    log_content = self._read_log(log_file)
                    compile_time = time.time() - start_time
                    result = CompileResult(
                        success=False,
                        errors=[CompileError(0, "PDF not generated after compilation")],
                        log=log_content,
                        compile_time=compile_time
                    )
                    return result
            
            # Read final log
            log_content = self._read_log(log_file)
            _, warnings = self._parse_log(log_content)
            
            # Count pages
            page_count = self._count_pages(log_content)
            
            compile_time = time.time() - start_time
            
            # Check for synctex file
            synctex_path = output_dir_path / "document.synctex.gz"
            synctex_file = str(synctex_path) if synctex_path.exists() else None
            
            # Save successful compilation to cache
            if pdf_path.exists():
                self._save_cache_entry(content_hash, pdf_path)
            
            result = CompileResult(
                success=True,
                pdf_path=str(pdf_path),
                warnings=warnings,
                log=log_content,
                compile_time=compile_time,
                page_count=page_count,
                synctex_path=synctex_file
            )
            return result
        finally:
            # Clean up temp directory if we created one
            if temp_dir_obj is not None:
                try:
                    temp_dir_obj.cleanup()
                except Exception:
                    pass
    
    async def compile_file_cached(
        self,
        tex_file: Path,
        output_dir: Optional[str] = None,
        max_iterations: int = 3
    ) -> CompileResult:
        """
        Compile a LaTeX file to PDF with proper multi-file caching.
        
        Uses bundle hash (main file + all dependencies) for cache key,
        ensuring cache invalidation when ANY file in the project changes.
        
        Args:
            tex_file: Path to main LaTeX file
            output_dir: Directory for output PDF (default: temp dir)
            max_iterations: Max compile passes (for cross-refs, TOC, etc.)
        
        Returns:
            CompileResult with success status, PDF path, and errors
        """
        import time
        start_time = time.time()
        
        # Check cache using bundle hash
        config = {'max_iterations': max_iterations}
        bundle_hash = await self._compute_bundle_hash(tex_file, config)
        
        cached_pdf = self._get_cached_pdf(bundle_hash)
        if cached_pdf:
            try:
                compile_time = time.time() - start_time
                # Return cached PDF without recompilation
                return CompileResult(
                    success=True,
                    pdf_path=str(cached_pdf),
                    compile_time=compile_time
                )
            except Exception:
                pass  # Fall through to regular compilation if cache read fails
        
        # Cache miss - perform actual compilation
        # Use synchronous compile_file for actual compilation
        from fastapi.concurrency import run_in_threadpool
        result = await run_in_threadpool(
            compile_file,
            str(tex_file),
            output_dir
        )
        
        # Save successful compilation to cache
        if result.success and result.pdf_path:
            pdf_path = Path(result.pdf_path)
            if pdf_path.exists():
                self._save_cache_entry(bundle_hash, pdf_path)
        
        return result
    
    def _run_pdflatex(self, tex_file: Path, output_dir: Path, job_id: Optional[str] = None) -> Dict:
        """Run pdflatex on a LaTeX file.
        
        Tracks the process for potential cancellation.
        
        Args:
            tex_file: Path to tex file
            output_dir: Output directory
            job_id: Optional job ID for tracking (allows concurrent job cancellation)
            
        Returns:
            Dict with returncode, stdout, stderr
        """
        try:
            # Use job_id or default to 'default' for backwards compatibility
            tracking_id = job_id or 'default'
            
            # Set resource limits on Unix systems
            def preexec_fn():
                import sys
                if sys.platform != 'win32':
                    try:
                        import resource
                        # Memory limit (512MB)
                        resource.setrlimit(resource.RLIMIT_AS, (MAX_COMPILE_MEMORY, MAX_COMPILE_MEMORY))
                        # CPU time limit (120s)
                        resource.setrlimit(resource.RLIMIT_CPU, (MAX_COMPILE_TIME, MAX_COMPILE_TIME))
                        # File size limit (100MB output)
                        resource.setrlimit(resource.RLIMIT_FSIZE, (100 * 1024 * 1024, 100 * 1024 * 1024))
                    except (ImportError, ValueError, OSError):
                        pass  # Resource limits not available
            
            # Use preexec_fn only on Unix, None on Windows
            import sys
            preexec = preexec_fn if sys.platform != 'win32' else None
            
            self._processes[tracking_id] = subprocess.Popen(
                [
                    "pdflatex",
                    "-interaction=nonstopmode",
                    "-halt-on-error",
                    "-no-shell-escape",  # Disable \write18 and other shell commands for security
                    "-synctex=1",  # Enable SyncTeX for PDF ↔ source jumping
                    f"-output-directory={output_dir}",
                    str(tex_file)
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                preexec_fn=preexec
            )
            
            try:
                stdout, stderr = self._processes[tracking_id].communicate(timeout=MAX_COMPILE_TIME)
                returncode = self._processes[tracking_id].returncode
            except subprocess.TimeoutExpired:
                # Kill process after timeout
                self._processes[tracking_id].kill()
                self._processes[tracking_id].wait()
                del self._processes[tracking_id]
                return {
                    'returncode': -1,
                    'stdout': '',
                    'stderr': f'pdflatex compilation timed out after {MAX_COMPILE_TIME}s'
                }
            finally:
                if tracking_id in self._processes:
                    del self._processes[tracking_id]
            
            return {
                'returncode': returncode,
                'stdout': stdout,
                'stderr': stderr
            }
        except FileNotFoundError:
            return {
                'returncode': -1,
                'stdout': '',
                'stderr': 'pdflatex not found'
            }
    
    def cancel_compilation(self, job_id: Optional[str] = None) -> bool:
        """Cancel a specific compilation process.
        
        Args:
            job_id: Optional job ID to cancel. If None, cancels 'default' job.
        
        Returns:
            True if process was terminated, False if no active process
        """
        tracking_id = job_id or 'default'
        
        if tracking_id not in self._processes:
            return False
        
        process = self._processes[tracking_id]
        if process.poll() is not None:
            # Process already finished
            del self._processes[tracking_id]
            return False
        
        try:
            # Try graceful termination first
            if os.name == 'nt':  # Windows
                os.kill(process.pid, signal.CTRL_C_EVENT)
            else:  # Unix/Linux/macOS
                process.terminate()
            
            # Wait briefly for graceful shutdown
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                # Force kill if still running
                process.kill()
                process.wait()
            
            return True
        except Exception:
            return False
        finally:
            if tracking_id in self._processes:
                del self._processes[tracking_id]
    
    def _read_log(self, log_file: Path) -> str:
        """Read pdflatex log file."""
        if log_file.exists():
            return log_file.read_text(encoding='utf-8', errors='ignore')
        return ""
    
    def _parse_log(self, log_content: str) -> Tuple[List[CompileError], List[CompileError]]:
        """
        Parse pdflatex log for errors and warnings with enhanced line detection.
        
        Recognizes patterns like:
        ! Undefined control sequence.
        l.123 some code here
        
        And:
        ./file.tex:42: LaTeX Error: ...
        
        Returns:
            Tuple of (errors, warnings)
        """
        errors = []
        warnings = []
        
        lines = log_content.split('\n')
        i = 0
        
        while i < len(lines):
            line = lines[i].strip()
            
            # Pattern 1: LaTeX error with line number
            # Matches: "! Error message" followed by "l.123 ..."
            if line.startswith("!"):
                message = line[1:].strip()  # Remove "!" prefix
                error_line = 0
                
                # Look ahead for line number (usually next 1-3 lines)
                for j in range(i + 1, min(i + 4, len(lines))):
                    next_line = lines[j].strip()
                    if next_line.startswith("l."):
                        # Extract line number and context
                        match = re.match(r"l\.(\d+)\s+(.*)", next_line)
                        if match:
                            error_line = int(match.group(1))
                            context = match.group(2)
                            message = f"{message} [context: {context}]"
                            break
                
                errors.append(CompileError(
                    line=error_line,
                    message=message,
                    error_type="error"
                ))
            
            # Pattern 2: File-specific error format
            # Matches: "./file.tex:42: Error message"
            file_error_match = re.match(r"^([./\w-]+\.tex):(\d+):\s*(.+?)$", line)
            if file_error_match:
                file_name = file_error_match.group(1)
                error_line = int(file_error_match.group(2))
                message = file_error_match.group(3)
                errors.append(CompileError(
                    line=error_line,
                    file=file_name,
                    message=message,
                    error_type="error"
                ))
            
            # Pattern 3: Warnings - various formats
            if "Undefined control sequence" in line:
                # Extract which command is undefined
                undef_match = re.search(r"Undefined control sequence.*?\\([a-zA-Z]+)", line)
                cmd = undef_match.group(1) if undef_match else "unknown"
                warnings.append(CompileError(
                    line=0,
                    message=f"Undefined control sequence: \\{cmd}",
                    error_type="warning"
                ))
            
            elif "Reference" in line and "undefined" in line.lower():
                warnings.append(CompileError(
                    line=0,
                    message=line,
                    error_type="undefined_reference"
                ))
            
            elif re.search(r"^(Warning|LaTeX Warning|Package Warning)", line):
                # Extract warning message
                warning_msg = re.sub(r"^(Warning|LaTeX Warning|Package Warning):\s*", "", line)
                warnings.append(CompileError(
                    line=0,
                    message=warning_msg,
                    error_type="warning"
                ))
            
            # Pattern 4: Overfull/Underfull boxes (badness warnings)
            elif re.search(r"(Overfull|Underfull).*hbox", line):
                warnings.append(CompileError(
                    line=0,
                    message=line,
                    error_type="warning"
                ))
            
            i += 1
        
        return errors, warnings
    
    def _count_pages(self, log_content: str) -> int:
        """Extract page count from log file."""
        # Look for "Output written on ... (N pages)"
        match = re.search(r"Output written on .+ \((\d+) pages?\)", log_content)
        if match:
            return int(match.group(1))
        return 0
    
    def cleanup(self, output_dir: Optional[str] = None) -> None:
        """Clean up temporary compilation files."""
        if output_dir:
            cleanup_dir = Path(output_dir)
        else:
            cleanup_dir = self.working_dir
        
        # Keep only PDF, delete aux files (never delete .tex)
        for ext in ['.aux', '.log', '.out', '.toc', '.lof', '.lot']:
            for f in cleanup_dir.glob(f"*{ext}"):
                try:
                    f.unlink()
                except Exception:
                    pass


# Convenience functions

def compile_tex(
    tex_content: str,
    output_dir: Optional[str] = None
) -> CompileResult:
    """Compile LaTeX content to PDF."""
    compiler = PDFCompiler()
    return compiler.compile(tex_content, output_dir)


def _copy_project_with_limits(
    main_file: Path,
    dest_root: Path,
    max_files: int = MAX_COPY_FILES,
    max_bytes: int = MAX_COPY_BYTES,
    ignore_dirs: Optional[Set[str]] = None
) -> Tuple[Optional[Path], Optional[CompileResult]]:
    """Copy project tree with safety limits to avoid runaway copies.

    Args:
        main_file: Path to the main LaTeX file
        dest_root: Destination root directory
        max_files: Maximum number of files to copy
        max_bytes: Maximum total bytes to copy
        ignore_dirs: Directory names to skip entirely

    Returns:
        Tuple of (copied_main_path, error_result). On success, error_result is None.
    """
    project_root = main_file.parent
    ignore_dirs = ignore_dirs or DEFAULT_IGNORE_DIRS
    copied_files = 0
    copied_bytes = 0

    for item in project_root.rglob("*"):
        try:
            rel_path = item.relative_to(project_root)
        except ValueError:
            continue

        # Skip ignored directories early
        if any(part in ignore_dirs for part in rel_path.parts):
            if item.is_dir():
                continue
            if item.is_file():
                continue

        if item.is_file():
            try:
                size = item.stat().st_size
            except OSError:
                continue

            if copied_files + 1 > max_files:
                msg = f"Project too large: exceeds file limit ({max_files} files)"
                return None, CompileResult(success=False, errors=[CompileError(0, msg, file=str(main_file))])

            if copied_bytes + size > max_bytes:
                msg = f"Project too large: exceeds {max_bytes // (1024 * 1024)}MB copy budget"
                return None, CompileResult(success=False, errors=[CompileError(0, msg, file=str(main_file))])

            dest_path = dest_root / rel_path
            dest_path.parent.mkdir(parents=True, exist_ok=True)
            try:
                shutil.copy2(item, dest_path)
            except OSError:
                continue

            copied_files += 1
            copied_bytes += size

    copied_main = dest_root / main_file.name
    if not copied_main.exists():
        msg = "Main file missing after project copy"
        return None, CompileResult(success=False, errors=[CompileError(0, msg, file=str(main_file))])

    return copied_main, None


def compile_file(
    tex_file: str,
    output_dir: Optional[str] = None
) -> CompileResult:
    """Compile a LaTeX file to PDF with multi-file support.
    
    This function handles projects with \\input, \\include, and dependencies.
    It copies the entire project structure to a temporary directory for compilation.
    
    Args:
        tex_file: Path to main LaTeX file
        output_dir: Directory for output PDF (default: same as tex_file)
        
    Returns:
        CompileResult with compilation status and PDF path
    """
    tex_path = Path(tex_file)
    if not tex_path.exists():
        return CompileResult(
            success=False,
            errors=[CompileError(0, f"File not found: {tex_file}")]
        )
    
    # Determine project root (directory containing the main file)
    project_root = tex_path.parent
    output_dir_path = Path(output_dir) if output_dir else project_root
    output_dir_path.mkdir(parents=True, exist_ok=True)
    
    # Read main content to check for dependencies
    content = tex_path.read_text(encoding='utf-8')
    
    # Check if project has dependencies (multi-file project)
    has_dependencies = _has_dependencies(content)
    
    if has_dependencies:
        # Multi-file project: copy into temp dir with safety limits, then compile
        temp_dir = Path(tempfile.mkdtemp(prefix="latex_ai_compile_", dir=tempfile.gettempdir()))
        try:
            copied_main, copy_error = _copy_project_with_limits(tex_path, temp_dir)
            if copy_error:
                return copy_error

            result = _compile_in_place(copied_main, output_dir_path)
            return result
        finally:
            try:
                shutil.rmtree(temp_dir, ignore_errors=True)
            except Exception:
                pass
    else:
        # Single file: compile in isolated temp dir then copy PDF to output_dir
        temp_dir = Path(tempfile.mkdtemp(prefix="latex_ai_compile_", dir=tempfile.gettempdir()))
        try:
            compiler = PDFCompiler(working_dir=str(temp_dir))
            result = compiler.compile(content, output_dir=temp_dir)

            if result.success and result.pdf_path:
                target_pdf = output_dir_path / f"{tex_path.stem}.pdf"
                target_pdf.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(result.pdf_path, target_pdf)
                result.pdf_path = str(target_pdf)

            return result
        finally:
            try:
                shutil.rmtree(temp_dir, ignore_errors=True)
            except Exception:
                pass


def _has_dependencies(content: str) -> bool:
    """Check if LaTeX content has file dependencies."""
    # Patterns for file inclusions
    patterns = [
        r'\\input\s*\{[^}]+\}',
        r'\\include\s*\{[^}]+\}',
        r'\\includegraphics\s*(?:\[[^\]]*\])?\s*\{[^}]+\}',
        r'\\bibliography\s*\{[^}]+\}',
        r'\\addbibresource\s*\{[^}]+\}'
    ]
    
    for pattern in patterns:
        if re.search(pattern, content):
            return True
    
    return False


def _compile_in_place(tex_path: Path, output_dir: Path) -> CompileResult:
    """Compile LaTeX file in its original directory to preserve dependencies.
    
    This approach keeps all \\input, \\include, and resource files accessible.
    After compilation, the PDF is moved to the desired output directory.
    
    Args:
        tex_path: Path to main LaTeX file
        output_dir: Desired output directory for PDF
        
    Returns:
        CompileResult
    """
    import time
    start_time = time.time()
    
    project_dir = tex_path.parent
    tex_filename = tex_path.name
    base_name = tex_path.stem
    
    # Compile in project directory
    compiler = PDFCompiler(working_dir=str(project_dir))
    
    errors = []
    warnings = []
    pdf_path = project_dir / f"{base_name}.pdf"
    log_file = project_dir / f"{base_name}.log"
    
    # Run pdflatex multiple times (for cross-refs)
    max_iterations = 3
    for iteration in range(max_iterations):
        result = subprocess.run(
            [
                "pdflatex",
                "-interaction=nonstopmode",
                "-halt-on-error",
                f"-output-directory={project_dir}",
                str(tex_path)
            ],
            capture_output=True,
            timeout=60,
            text=True,
            cwd=str(project_dir)  # Run in project directory
        )
        
        if result.returncode != 0 and iteration == max_iterations - 1:
            # Parse log for errors
            log_content = log_file.read_text(encoding='utf-8', errors='ignore') if log_file.exists() else ""
            errors, warnings = compiler._parse_log(log_content)
            
            compile_time = time.time() - start_time
            return CompileResult(
                success=False,
                errors=errors,
                warnings=warnings,
                log=log_content,
                compile_time=compile_time
            )
    
    # Check if PDF was generated
    if not pdf_path.exists():
        log_content = log_file.read_text(encoding='utf-8', errors='ignore') if log_file.exists() else ""
        compile_time = time.time() - start_time
        return CompileResult(
            success=False,
            errors=[CompileError(0, "PDF not generated after compilation")],
            log=log_content,
            compile_time=compile_time
        )
    
    # Move PDF to output directory if different
    final_pdf_path = pdf_path
    if output_dir != project_dir:
        final_pdf_path = output_dir / f"{base_name}.pdf"
        shutil.copy2(pdf_path, final_pdf_path)
    
    # Read log
    log_content = log_file.read_text(encoding='utf-8', errors='ignore') if log_file.exists() else ""
    _, warnings = compiler._parse_log(log_content)
    page_count = compiler._count_pages(log_content)
    
    compile_time = time.time() - start_time
    # Clean up aux artifacts but keep pdf
    try:
        compiler.cleanup(project_dir)
    except Exception:
        pass
    
    return CompileResult(
        success=True,
        pdf_path=str(final_pdf_path),
        warnings=warnings,
        log=log_content,
        compile_time=compile_time,
        page_count=page_count
    )
