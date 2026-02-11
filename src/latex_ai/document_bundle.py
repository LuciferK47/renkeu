"""Parse and bundle LaTeX documents with dependencies (\\input, \\include, \\bibliography)."""

import re
import asyncio
import os
import hashlib
from pathlib import Path
from typing import Dict, Set, Optional, Tuple

try:
    import aiofiles
    import aiofiles.os
    HAS_AIOFILES = True
except ImportError:
    HAS_AIOFILES = False


# Global cache for bundle combined text
# Key: mtime_hash, Value: (combined_text, timestamp)
_bundle_cache: Dict[str, Tuple[str, float]] = {}
_cache_max_size = 100
_cache_max_age = 300  # 5 minutes


# Async helpers with aiofiles fallback
async def _astat(path: str):
    """Get file stats asynchronously, with fallback to asyncio.to_thread."""
    if HAS_AIOFILES:
        return await aiofiles.os.stat(path)
    else:
        return await asyncio.to_thread(os.stat, path)


async def _aread_text(path: str) -> str:
    """Read file asynchronously, with fallback to asyncio.to_thread."""
    if HAS_AIOFILES:
        async with aiofiles.open(path, mode="r", encoding="utf-8") as f:
            return await f.read()
    else:
        return await asyncio.to_thread(Path(path).read_text, encoding="utf-8")


class DocumentBundle:
    """Represents a LaTeX document and all its dependencies.
    
    Features:
    - Loads \input, \include, \bibliography, \addbibresource
    - Detects circular dependencies and reports them
    - Tracks \usepackage declarations for package dependencies
    - Limits recursion depth and file count to prevent DoS
    """
    
    def __init__(
        self, 
        main_file: Path, 
        workspace_root: Optional[Path] = None,
        max_depth: int = 10,
        max_files: int = 50,
    ):
        """Initialize document bundle.
        
        Args:
            main_file: Path to main LaTeX file
            workspace_root: Root directory for relative imports
            max_depth: Maximum recursion depth for dependencies (prevents infinite loops)
            max_files: Maximum number of files to load (prevents DoS)
        """
        self.main_file = Path(main_file)
        self.workspace_root = Path(workspace_root or self.main_file.parent)
        self.max_depth = max_depth
        self.max_files = max_files
        
        self.files: Dict[str, str] = {}
        self.dependencies: Dict[str, Set[Path]] = {}
        self.packages: Dict[str, Optional[str]] = {}  # package_name -> options
        self.circular_deps: list[list[str]] = []  # List of circular dependency chains detected
        self.errors: list[str] = []
        self.warnings: list[str] = []
    
    def _detect_circular_dependencies(self) -> list[list[str]]:
        """Detect circular dependencies in the loaded bundle.
        
        Returns:
            List of circular dependency chains (each chain is list of file paths)
        """
        circular_chains = []
        visited = set()
        rec_stack = set()
        
        def dfs(node: str, path: list[str]) -> bool:
            """DFS to detect cycles.
            
            Returns:
                True if cycle detected
            """
            visited.add(node)
            rec_stack.add(node)
            path.append(node)
            
            deps = self.dependencies.get(node, set())
            for dep in deps:
                dep_str = str(dep)
                if dep_str not in visited:
                    if dfs(dep_str, path.copy()):
                        return True
                elif dep_str in rec_stack:
                    # Cycle found
                    cycle_start = path.index(dep_str)
                    circular_chains.append(path[cycle_start:] + [dep_str])
                    return True
            
            rec_stack.remove(node)
            return False
        
        # Check from each node
        for node in self.dependencies.keys():
            if node not in visited:
                dfs(node, [])
        
        return circular_chains
        max_files: int = 50
    ):
        """Initialize with main file path.
        
        Args:
            main_file: Path to main LaTeX file
            workspace_root: Explicit workspace root for security (defaults to main_file.parent)
            max_depth: Maximum recursion depth for dependencies (default 10)
            max_files: Maximum total files to load (default 50)
        """
        self.main_file = main_file
        # Use explicit workspace_root when provided; default to main file's parent
        self.workspace_root = (workspace_root or main_file.parent).resolve()
        self.max_depth = max_depth
        self.max_files = max_files
        self.files: Dict[str, str] = {}  # file_path -> content
        self.dependencies: Dict[str, Set[str]] = {}  # file_path -> set of dependency paths
        self.errors: list = []
        
    async def load_async(self) -> bool:
        """Asynchronously load main file and all dependencies.
        
        Detects circular dependencies after loading.
        
        Returns:
            True if main file loaded, False if any critical error.
        """
        from .config import get_config
        
        try:
            # Read main file with size limit
            cfg = get_config()
            stat = await _astat(str(self.main_file))
            if stat.st_size > cfg.max_file_size:
                raise ValueError("Main file exceeds size limit")
            
            main_content = await _aread_text(str(self.main_file))
            
            self.files[str(self.main_file)] = main_content
            await self._load_dependencies_async(self.main_file, set(), depth=0)
            
            # Check for circular dependencies
            self.circular_deps = self._detect_circular_dependencies()
            if self.circular_deps:
                for cycle in self.circular_deps:
                    cycle_str = " -> ".join(cycle)
                    self.warnings.append(f"Circular dependency detected: {cycle_str}")
            
            return True
        except Exception as e:
            self.errors.append(f"Failed to load main file: {e}")
            return False
    
    async def _load_dependencies_async(self, file_path: Path, visited: Set[Path], depth: int = 0) -> None:
        """Recursively load dependencies.
        
        Args:
            file_path: Path to file to scan
            visited: Set of already-visited paths (prevent cycles)
            depth: Current recursion depth
        """
        from .config import get_config
        
        # Check depth limit
        if depth > self.max_depth:
            self.errors.append(f"Max depth {self.max_depth} exceeded at {file_path}")
            return
        
        # Check file count limit
        if len(visited) > self.max_files:
            self.errors.append(f"Max files {self.max_files} exceeded")
            return
        
        if file_path in visited:
            return
        visited.add(file_path)
        
        try:
            # Enforce size limit for dependencies
            cfg = get_config()
            stat = await _astat(str(file_path))
            if stat.st_size > cfg.max_file_size:
                self.errors.append(f"Skipped {file_path}: exceeds size limit")
                return
            content = await _aread_text(str(file_path))
        except Exception as e:
            self.errors.append(f"Failed to load {file_path}: {e}")
            return
        
        # Find all referenced files
        deps = self._parse_dependencies(content, file_path)
        self.dependencies[str(file_path)] = deps
        
        # Recursively load each dependency
        for dep_path in deps:
            if dep_path not in visited:
                await self._load_dependencies_async(dep_path, visited, depth + 1)
                if dep_path not in self.files:
                    try:
                        # Enforce size limit for dependency file content as well
                        cfg = get_config()
                        stat = await _astat(str(dep_path))
                        if stat.st_size > cfg.max_file_size:
                            self.errors.append(f"Skipped {dep_path}: exceeds size limit")
                        else:
                            content = await _aread_text(str(dep_path))
                            self.files[str(dep_path)] = content
                    except Exception as e:
                        self.errors.append(f"Failed to load dependency {dep_path}: {e}")
    
    def _parse_dependencies(self, content: str, base_file: Path) -> Set[Path]:
        """Parse LaTeX content for \input, \include, \bibliography references.
        
        Also extracts \usepackage declarations for package tracking.
        
        Args:
            content: LaTeX file content
            base_file: Path to the file being parsed
        
        Returns:
            Set of absolute paths to dependencies
        """
        deps = set()
        
        # Extract \usepackage declarations
        # Matches: \usepackage{package}, \usepackage[options]{package}
        usepackage_pattern = r'\\usepackage(?:\[([^\]]*)\])?\{([^\}]+)\}'
        for match in re.finditer(usepackage_pattern, content):
            options = match.group(1) or None
            packages = match.group(2).split(',')
            for pkg in packages:
                pkg = pkg.strip()
                self.packages[pkg] = options
        
        # Match \input{file} or \input file
        input_pattern = r'\\input\s*\{?([^\}]+)\}?'
        for match in re.finditer(input_pattern, content):
            dep_file = match.group(1).strip()
            if '{' in dep_file:
                continue  # Skip malformed
            dep_path = self._resolve_file(base_file, dep_file)
            if dep_path and dep_path.exists():
                deps.add(dep_path)
        
        # Match \include{file}
        include_pattern = r'\\include\s*\{([^\}]+)\}'
        for match in re.finditer(include_pattern, content):
            dep_file = match.group(1).strip()
            dep_path = self._resolve_file(base_file, dep_file)
            if dep_path and dep_path.exists():
                deps.add(dep_path)
        
        # Match \addbibresource{file.bib} (BibLaTeX)
        addbib_pattern = r'\\addbibresource\s*\{([^\}]+)\}'
        for match in re.finditer(addbib_pattern, content):
            dep_file = match.group(1).strip()
            dep_path = self._resolve_file(base_file, dep_file)
            if dep_path and dep_path.exists():
                deps.add(dep_path)
        
        # Match \bibliography{file} or \bibliography{file1,file2,...}
        bib_pattern = r'\\bibliography\s*\{([^\}]+)\}'
        for match in re.finditer(bib_pattern, content):
            bib_files = match.group(1).split(",")
            for bib_file in bib_files:
                bib_file = bib_file.strip()
                # Try with .bib extension if not present
                if not bib_file.endswith(".bib"):
                    bib_file += ".bib"
                bib_path = self._resolve_file(base_file, bib_file)
                if bib_path and bib_path.exists():
                    deps.add(bib_path)
        
        return deps
    
    def _resolve_file(self, base_file: Path, relative_path: str) -> Optional[Path]:
        """Resolve a file path relative to a base file.
        
        Tries multiple extensions (.tex, .bib, none).
        
        Args:
            base_file: Base file path
            relative_path: Relative or absolute path to resolve
        
        Returns:
            Resolved absolute Path or None if not found
        """
        base_dir = base_file.parent

        # Reject absolute paths outright
        try:
            if Path(relative_path).is_absolute():
                return None
        except Exception:
            return None

        def _inside_workspace(p: Path) -> bool:
            try:
                p.resolve().relative_to(self.workspace_root)
                return True
            except Exception:
                return False

        # Helper to validate candidate
        def _valid_file(p: Path) -> Optional[Path]:
            p = p.resolve()
            if p.exists() and p.is_file() and _inside_workspace(p):
                return p
            return None

        # Try direct path first
        candidate = _valid_file(base_dir / relative_path)
        if candidate:
            return candidate

        # Try with .tex extension
        candidate = _valid_file(base_dir / f"{relative_path}.tex")
        if candidate:
            return candidate

        # Try with .bib extension (for bibliography files)
        candidate = _valid_file(base_dir / f"{relative_path}.bib")
        if candidate:
            return candidate
        
        # For bibliography files, also try from workspace root with .bib
        if not relative_path.endswith('.bib'):
            candidate = _valid_file(self.workspace_root / f"{relative_path}.bib")
            if candidate:
                return candidate

        # Try from workspace root
        candidate = _valid_file(self.workspace_root / relative_path)
        if candidate:
            return candidate

        return None
    
    def as_dict(self) -> Dict:
        """Return bundle as dictionary for JSON serialization.
        
        Returns:
            Dict with keys: main_file, files (path -> content), dependencies
        """
        return {
            "main_file": str(self.main_file),
            "files": {k: v for k, v in self.files.items()},
            "dependencies": {k: list(v) for k, v in self.dependencies.items()},
            "errors": self.errors
        }
    
    def get_combined_text(self) -> str:
        """Get all content concatenated with file markers.
        
        Caches the result based on file mtimes. If files haven't changed,
        returns cached combined text to avoid rebuilding on every AI request.
        
        Useful for passing to AI with context of what's in each file.
        
        Returns:
            String with all files joined with clear delimiters
        """
        import time
        
        # Compute mtime hash for caching
        mtime_hash = self._compute_mtime_hash()
        current_time = time.time()
        
        # Check cache
        if mtime_hash in _bundle_cache:
            cached_text, cached_time = _bundle_cache[mtime_hash]
            # Return cached if not expired
            if current_time - cached_time < _cache_max_age:
                return cached_text
        
        # Build combined text
        result = []
        # Start with main file
        result.append(f"=== {self.main_file.name} ===\n")
        result.append(self.files.get(str(self.main_file), ""))
        
        # Then add dependencies in order
        for file_path in sorted(self.files.keys()):
            if file_path != str(self.main_file):
                path_obj = Path(file_path)
                result.append(f"\n=== {path_obj.name} ===\n")
                result.append(self.files[file_path])
        
        combined = "\n".join(result)
        
        # Update cache with LRU eviction
        if len(_bundle_cache) >= _cache_max_size:
            # Remove oldest entry
            oldest_key = min(_bundle_cache.keys(), 
                           key=lambda k: _bundle_cache[k][1])
            del _bundle_cache[oldest_key]
        
        _bundle_cache[mtime_hash] = (combined, current_time)
        return combined
    
    def _compute_mtime_hash(self) -> str:
        """Compute hash based on file modification times.
        
        Returns:
            SHA256 hash of all file paths + mtimes
        """
        h = hashlib.sha256()
        for file_path in sorted(self.files.keys()):
            try:
                mtime = os.path.getmtime(file_path)
                h.update(f"{file_path}:{mtime}".encode())
            except (OSError, FileNotFoundError):
                # File deleted or inaccessible, use path only
                h.update(file_path.encode())
        return h.hexdigest()[:16]  # 16 chars sufficient

