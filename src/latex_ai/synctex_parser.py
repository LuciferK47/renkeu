"""SyncTeX parser for bidirectional PDF ↔ LaTeX source navigation."""

import gzip
import hashlib
import re
from pathlib import Path
from typing import Optional, Tuple, List, Dict
from dataclasses import dataclass

# Global cache for parsed SyncTeX data
# Key: PDF file hash, Value: parsed mappings
_synctex_cache: Dict[str, 'SyncTeXData'] = {}


@dataclass
class SyncTeXMapping:
    """A mapping between PDF page/position and source file/line."""
    page: int
    h: float  # Horizontal position
    v: float  # Vertical position
    file: str
    line: int
    column: int = 0


class SyncTeXParser:
    """Parse SyncTeX .gz files for PDF-to-source synchronization."""
    
    def __init__(self, synctex_file: Path):
        """Initialize parser with synctex file.
        
        Args:
            synctex_file: Path to .synctex.gz file
            
        Raises:
            FileNotFoundError: If synctex file doesn't exist
        """
        if not synctex_file.exists():
            raise FileNotFoundError(f"SyncTeX file not found: {synctex_file}")
        
        self.synctex_file = synctex_file
        self._mappings: List[SyncTeXMapping] = []
        self._file_map: Dict[int, str] = {}  # file_id -> file_path
        self._parse()
    
    def _parse(self):
        """Parse the synctex.gz file and build mappings."""
        with gzip.open(self.synctex_file, 'rt', encoding='utf-8') as f:
            content = f.read()
        
        # Parse file input records (Input:id:file)
        for match in re.finditer(r'Input:(\d+):(.+)', content):
            file_id = int(match.group(1))
            file_path = match.group(2).strip()
            self._file_map[file_id] = file_path
        
        # Parse position records (more complex, simplified for POC)
        # Full implementation would parse the binary-like format
        # For now, we'll use a regex-based approach for text content
        
        current_page = 1
        current_file = 1
        
        for line in content.split('\n'):
            # Page markers
            if line.startswith('{'):
                page_match = re.search(r'\[(\d+)', line)
                if page_match:
                    current_page = int(page_match.group(1))
            
            # Content markers (simplified)
            if line.startswith('x') or line.startswith('g') or line.startswith('k'):
                # Parse position and line number
                # Format: x<h>,<v>:<file>:<line>
                parts = re.match(r'[xgk](-?\d+),(-?\d+):(\d+):(\d+)', line)
                if parts:
                    h = float(parts.group(1))
                    v = float(parts.group(2))
                    file_id = int(parts.group(3))
                    line_num = int(parts.group(4))
                    
                    if file_id in self._file_map:
                        self._mappings.append(SyncTeXMapping(
                            page=current_page,
                            h=h,
                            v=v,
                            file=self._file_map[file_id],
                            line=line_num
                        ))
    
    def forward_search(self, source_file: str, line: int) -> Optional[Dict]:
        """Find PDF location from source file and line (forward sync).
        
        Args:
            source_file: Path to source .tex file
            line: Line number in source file
            
        Returns:
            Dict with page, x, y positions or None
        """
        # Find closest mapping for this file and line
        source_file = str(Path(source_file).resolve())
        
        best_match = None
        min_distance = float('inf')
        
        for mapping in self._mappings:
            if Path(mapping.file).resolve() == Path(source_file):
                distance = abs(mapping.line - line)
                if distance < min_distance:
                    min_distance = distance
                    best_match = mapping
        
        if best_match:
            return {
                'page': best_match.page,
                'x': best_match.h,
                'y': best_match.v,
                'file': best_match.file,
                'line': best_match.line
            }
        
        return None
    
    def backward_search(self, page: int, x: float, y: float) -> Optional[Dict]:
        """Find source location from PDF position (backward sync).
        
        Args:
            page: PDF page number
            x: Horizontal position on page
            y: Vertical position on page
            
        Returns:
            Dict with file and line or None
        """
        # Find closest mapping for this page and position
        best_match = None
        min_distance = float('inf')
        
        for mapping in self._mappings:
            if mapping.page == page:
                # Calculate distance (simplified Euclidean)
                distance = ((mapping.h - x) ** 2 + (mapping.v - y) ** 2) ** 0.5
                if distance < min_distance:
                    min_distance = distance
                    best_match = mapping
        
        if best_match:
            return {
                'file': best_match.file,
                'line': best_match.line,
                'column': best_match.column,
                'page': best_match.page
            }
        
        return None
    
    def get_all_mappings(self) -> List[SyncTeXMapping]:
        """Get all parsed mappings.
        
        Returns:
            List of all SyncTeX mappings
        """
        return self._mappings


def parse_synctex(synctex_file: Path) -> SyncTeXParser:
    """Convenience function to create a SyncTeX parser.
    
    Args:
        synctex_file: Path to .synctex.gz file
        
    Returns:
        SyncTeXParser instance
    """
    return SyncTeXParser(synctex_file)
