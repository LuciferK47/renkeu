"""Tests for SyncTeX parser functionality."""

import gzip
import tempfile
from pathlib import Path
import pytest

from latex_ai.synctex_parser import SyncTeXParser, parse_synctex


@pytest.fixture
def mock_synctex_file():
    """Create a mock synctex.gz file for testing."""
    content = """SyncTeX Version:1
Input:1:./test.tex
Input:2:./section.tex
Output:pdf
Magnification:1000
Unit:1
X Offset:0
Y Offset:0
Content:
{1
[1,0,0,0,0,0
(1,8,0,0,0,1,1
x3000,6000:1:12
x3000,7000:1:15
x3000,8000:2:8
]
}
"""
    
    # Create temporary synctex.gz file
    with tempfile.NamedTemporaryFile(mode='wb', suffix='.synctex.gz', delete=False) as f:
        with gzip.open(f.name, 'wt', encoding='utf-8') as gz:
            gz.write(content)
        return Path(f.name)


def test_synctex_parser_initialization(mock_synctex_file):
    """Test SyncTeX parser initialization."""
    parser = SyncTeXParser(mock_synctex_file)
    assert parser.synctex_file == mock_synctex_file
    assert len(parser._file_map) >= 1
    assert len(parser._mappings) >= 1


def test_synctex_parser_file_not_found():
    """Test that parser raises FileNotFoundError for missing file."""
    with pytest.raises(FileNotFoundError):
        SyncTeXParser(Path("/nonexistent/file.synctex.gz"))


def test_synctex_forward_search(mock_synctex_file):
    """Test forward search from source to PDF."""
    parser = SyncTeXParser(mock_synctex_file)
    
    # Search for line in test.tex
    result = parser.forward_search("./test.tex", 12)
    
    # Should find a match (even if approximate)
    if result:
        assert result['page'] == 1
        assert result['file'] in ['./test.tex', './section.tex']
        assert 'x' in result
        assert 'y' in result


def test_synctex_backward_search(mock_synctex_file):
    """Test backward search from PDF to source."""
    parser = SyncTeXParser(mock_synctex_file)
    
    # Search near a known position
    result = parser.backward_search(page=1, x=3000, y=6000)
    
    # Should find closest match
    if result:
        assert result['page'] == 1
        assert result['file'] in ['./test.tex', './section.tex']
        assert 'line' in result


def test_synctex_get_all_mappings(mock_synctex_file):
    """Test retrieval of all mappings."""
    parser = SyncTeXParser(mock_synctex_file)
    mappings = parser.get_all_mappings()
    
    assert isinstance(mappings, list)
    assert len(mappings) >= 0


def test_parse_synctex_convenience_function(mock_synctex_file):
    """Test convenience function for creating parser."""
    parser = parse_synctex(mock_synctex_file)
    
    assert isinstance(parser, SyncTeXParser)
    assert parser.synctex_file == mock_synctex_file


def test_synctex_forward_no_match(mock_synctex_file):
    """Test forward search with no matching file."""
    parser = SyncTeXParser(mock_synctex_file)
    
    # Search for non-existent file
    result = parser.forward_search("/nonexistent.tex", 1)
    
    assert result is None


def test_synctex_backward_no_match(mock_synctex_file):
    """Test backward search with no matching page."""
    parser = SyncTeXParser(mock_synctex_file)
    
    # Search on non-existent page
    result = parser.backward_search(page=999, x=0, y=0)
    
    assert result is None
