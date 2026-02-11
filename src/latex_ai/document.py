"""Document abstraction layer for LaTeX files.

Provides semantic operations on LaTeX documents, hiding low-level text manipulation.
"""
from __future__ import annotations
import re
from pathlib import Path
from typing import List, Optional, Tuple, Dict
from dataclasses import dataclass

try:
    import aiofiles
    HAS_AIOFILES = True
except ImportError:
    HAS_AIOFILES = False

from .parser import LatexParser


@dataclass
class DocumentPosition:
    """Represents a position in a LaTeX document."""
    line: int
    column: int
    offset: int  # Character offset from start


@dataclass
class DocumentRange:
    """Represents a range in a LaTeX document."""
    start: DocumentPosition
    end: DocumentPosition


class Document:
    """High-level abstraction for LaTeX document manipulation.
    
    Provides semantic methods for common LaTeX operations while maintaining
    a clean separation from the underlying text representation.
    """
    
    def __init__(self, path: Path, text: Optional[str] = None, use_ast: bool = True) -> None:
        self.path = Path(path)
        self.text = text or ""  # Will be loaded async if None
        self._cached_packages: Optional[List[str]] = None
        self.use_ast = use_ast and LatexParser.is_available()
        self._parser: Optional[LatexParser] = None
    
    @classmethod
    async def load(cls, path: Path, use_ast: bool = True) -> 'Document':
        """Load document asynchronously from disk.
        
        Args:
            path: Path to LaTeX file
            use_ast: Whether to use AST parsing
            
        Returns:
            Document instance with content loaded
        """
        if HAS_AIOFILES:
            async with aiofiles.open(path, mode="r", encoding="utf-8") as f:
                text = await f.read()
        else:
            # Fallback to sync for compatibility
            text = path.read_text(encoding="utf-8")
        
        return cls(path=path, text=text, use_ast=use_ast)
    
    def _get_parser(self) -> LatexParser:
        """Get or create parser for current text."""
        if self._parser is None or self._parser.text != self.text:
            self._parser = LatexParser(self.text, use_ast=self.use_ast)
        return self._parser
    
    async def reload(self) -> None:
        """Reload document from disk asynchronously."""
        if HAS_AIOFILES:
            async with aiofiles.open(self.path, mode="r", encoding="utf-8") as f:
                self.text = await f.read()
        else:
            # Fallback to sync for compatibility
            self.text = self.path.read_text(encoding="utf-8")
        self._cached_packages = None
        self._parser = None  # Invalidate parser cache
    
    # Structure detection
    
    def find_documentclass(self) -> Optional[Tuple[str, Optional[str]]]:
        """Find document class and options.
        
        Returns:
            Tuple of (class_name, options) or None
        """
        parser = self._get_parser()
        return parser.find_documentclass()
    
    def find_preamble_end(self) -> int:
        """Find character offset where preamble ends (before \\begin{document}).
        
        Returns:
            Character offset, or 0 if no \\begin{document} found
        """
        parser = self._get_parser()
        return parser.find_preamble_end()
    
    def find_documentclass_end(self) -> int:
        """Find character offset after \\documentclass line.
        
        Returns:
            Character offset, or 0 if no documentclass found
        """
        pattern = re.compile(
            r"^\\documentclass\s*(\[[^\]]*\])?\s*\{[^}]+\}",
            re.MULTILINE
        )
        match = pattern.search(self.text)
        return match.end() if match else 0
    
    def get_packages(self) -> List[Tuple[str, Optional[str]]]:
        """Get all loaded packages with their options.
        
        Returns:
            List of (package_name, options) tuples
        """
        if self._cached_packages is not None:
            return self._cached_packages
        
        parser = self._get_parser()
        packages = parser.find_packages()
        
        self._cached_packages = packages
        return packages
    
    def has_package(self, package_name: str) -> bool:
        """Check if package is loaded."""
        packages = self.get_packages()
        return any(pkg == package_name for pkg, _ in packages)
    
    def find_environment(self, env_name: str) -> Optional[DocumentRange]:
        """Find a LaTeX environment.
        
        Args:
            env_name: Environment name (e.g., 'document', 'figure')
            
        Returns:
            DocumentRange spanning the environment, or None
        """
        parser = self._get_parser()
        result = parser.find_environment(env_name)
        if result:
            start_offset, end_offset = result
            start = self._offset_to_position(start_offset)
            end = self._offset_to_position(end_offset)
            return DocumentRange(start, end)
        return None
    
    # Semantic edit operations
    
    def insert_preamble(self, content: str) -> str:
        """Insert content into preamble (after documentclass).
        
        Returns:
            New document text
        """
        insert_pos = self.find_documentclass_end()
        return self.text[:insert_pos] + f"\n{content}\n" + self.text[insert_pos:]
    
    def insert_after_begin_document(self, content: str) -> str:
        """Insert content right after \\begin{document}.
        
        Returns:
            New document text
        """
        match = re.search(r"\\begin\{document\}", self.text)
        if match:
            insert_pos = match.end()
            return self.text[:insert_pos] + f"\n{content}\n" + self.text[insert_pos:]
        else:
            # Fallback: prepend to document
            return content + "\n" + self.text
    
    def replace_documentclass(self, new_class: str, options: Optional[str] = None) -> str:
        """Replace documentclass.
        
        Args:
            new_class: New document class name
            options: Optional class options (if None, preserves existing)
            
        Returns:
            New document text
        """
        pattern = re.compile(
            r"^\\documentclass\s*(\[(?P<opts>[^\]]*)\])?\s*\{(?P<class>[^}]+)\}",
            re.MULTILINE
        )
        match = pattern.search(self.text)
        if not match:
            return self.text
        
        if options is None:
            options = match.group("opts")
        
        if options:
            replacement = f"\\documentclass[{options}]{{{new_class}}}"
        else:
            replacement = f"\\documentclass{{{new_class}}}"
        
        return self.text[:match.start()] + replacement + self.text[match.end():]
    
    def add_package(self, package_name: str, options: Optional[str] = None) -> str:
        """Add package to preamble if not already present.
        
        Returns:
            New document text
        """
        if self.has_package(package_name):
            return self.text
        
        if options:
            pkg_line = f"\\usepackage[{options}]{{{package_name}}}"
        else:
            pkg_line = f"\\usepackage{{{package_name}}}"
        
        return self.insert_preamble(pkg_line)
    
    def remove_package(self, package_name: str) -> str:
        """Remove package from preamble.
        
        Returns:
            New document text
        """
        pattern = re.compile(
            rf"\\usepackage\s*(\[[^\]]*\])?\s*\{{{re.escape(package_name)}\}}\n?",
            re.MULTILINE
        )
        return pattern.sub("", self.text)
    
    def update_package_option(self, package_name: str, key: str, value: str) -> str:
        """Update or add an option to an existing package.
        
        Returns:
            New document text, or original if package not found
        """
        pattern = re.compile(
            rf"\\usepackage\s*(\[(?P<opts>[^\]]*)\])?\s*\{{{re.escape(package_name)}\}}",
            re.MULTILINE
        )
        
        def replace_opts(match: re.Match) -> str:
            opts_str = match.group("opts") or ""
            opts_dict = self._parse_options(opts_str)
            opts_dict[key] = value
            new_opts = self._format_options(opts_dict)
            return f"\\usepackage[{new_opts}]{{{package_name}}}"
        
        return pattern.sub(replace_opts, self.text, count=1)
    
    # Utility methods
    
    def _offset_to_position(self, offset: int) -> DocumentPosition:
        """Convert character offset to line/column position."""
        text_before = self.text[:offset]
        lines = text_before.split("\n")
        line = len(lines)
        column = len(lines[-1]) + 1
        return DocumentPosition(line, column, offset)
    
    @staticmethod
    def _parse_options(opts_str: str, sep: str = ",", kvsep: str = "=") -> Dict[str, str]:
        """Parse LaTeX package options into dict."""
        result: Dict[str, str] = {}
        for part in [o.strip() for o in opts_str.split(sep) if o.strip()]:
            if kvsep in part:
                k, v = part.split(kvsep, 1)
                result[k.strip()] = v.strip()
            else:
                result[part] = "true"
        return result
    
    @staticmethod
    def _format_options(opts_dict: Dict[str, str], sep: str = ",", kvsep: str = "=") -> str:
        """Format options dict back to LaTeX string."""
        items = []
        for k, v in opts_dict.items():
            if v == "true":
                items.append(k)
            else:
                items.append(f"{k}{kvsep}{v}")
        return sep.join(items)
