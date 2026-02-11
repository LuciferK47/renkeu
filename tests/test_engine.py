"""Unit tests for LatexEngine."""
import pytest
from pathlib import Path
from latex_ai.engine import LatexEngine, EditResult


@pytest.fixture
def sample_tex(tmp_path):
    """Create a temporary .tex file for testing."""
    tex_file = tmp_path / "test.tex"
    content = r"""\documentclass{article}
\usepackage{graphicx}

\title{Test Document}
\author{Test Author}

\begin{document}
\maketitle

This is a test document.

\begin{figure}[h]
  \centering
  \includegraphics{example.png}
  \caption{Test figure}
\end{figure}

\end{document}
"""
    tex_file.write_text(content, encoding="utf-8")
    return tex_file


@pytest.fixture
def engine(sample_tex):
    """Create a LatexEngine instance for testing."""
    return LatexEngine(sample_tex)


class TestEngineBasics:
    """Test basic engine functionality."""
    
    def test_engine_initialization(self, sample_tex):
        """Test that engine loads file correctly."""
        engine = LatexEngine(sample_tex)
        assert engine.path == sample_tex
        assert "\\documentclass{article}" in engine.text
        assert engine._max_backups == 10
    
    def test_reload(self, engine, sample_tex):
        """Test that reload updates text from disk."""
        # Modify file externally
        sample_tex.write_text("\\documentclass{book}", encoding="utf-8")
        engine.reload()
        assert engine.text == "\\documentclass{book}"
    
    def test_preview(self, engine):
        """Test preview extracts documentclass and geometry."""
        info = engine.preview()
        assert info["documentclass"] == "article"
        assert info["geometry"] == "(not found)"


class TestSetMargins:
    """Test set margins command."""
    
    def test_set_margins_adds_geometry(self, engine):
        """Test adding geometry package when not present."""
        result = engine.apply_command("set margins 1in", auto_save=False)
        assert result.changed
        assert "\\usepackage[margin=1in]{geometry}" in result.new_text
        assert "Set geometry margin to 1in" in result.message
    
    def test_set_margins_updates_existing(self, sample_tex):
        """Test updating existing geometry package."""
        content = r"""\documentclass{article}
\usepackage[margin=2cm]{geometry}
\begin{document}
Test
\end{document}
"""
        sample_tex.write_text(content, encoding="utf-8")
        engine = LatexEngine(sample_tex)
        result = engine.apply_command("set margins 1.5in", auto_save=False)
        assert result.changed
        assert "margin=1.5in" in result.new_text
        assert "margin=2cm" not in result.new_text
    
    def test_set_margins_invalid_format(self, engine):
        """Test validation rejects invalid margin format."""
        result = engine.apply_command("set margins banana", auto_save=False)
        assert not result.changed
        assert "Invalid size format" in result.message
    
    def test_set_margins_valid_formats(self, engine):
        """Test various valid margin formats."""
        valid_formats = ["1in", "2.5cm", "10mm", "12pt"]
        for fmt in valid_formats:
            result = engine.apply_command(f"set margins {fmt}", auto_save=False)
            assert result.changed, f"Format {fmt} should be valid"


class TestSetLineSpacing:
    """Test set line spacing command."""
    
    def test_set_line_spacing_single(self, engine):
        """Test setting single spacing."""
        result = engine.apply_command("set line spacing 1", auto_save=False)
        assert result.changed
        assert "\\usepackage{setspace}" in result.new_text
        assert "\\singlespacing" in result.new_text
    
    def test_set_line_spacing_onehalf(self, engine):
        """Test setting 1.5 spacing."""
        result = engine.apply_command("set line spacing 1.5", auto_save=False)
        assert result.changed
        assert "\\onehalfspacing" in result.new_text
    
    def test_set_line_spacing_double(self, engine):
        """Test setting double spacing."""
        result = engine.apply_command("set line spacing 2", auto_save=False)
        assert result.changed
        assert "\\doublespacing" in result.new_text
    
    def test_set_line_spacing_custom(self, engine):
        """Test custom spacing value."""
        result = engine.apply_command("set line spacing 1.8", auto_save=False)
        assert result.changed
        assert "\\setstretch{1.8}" in result.new_text
    
    def test_set_line_spacing_invalid(self, engine):
        """Test validation rejects out-of-range spacing."""
        result = engine.apply_command("set line spacing 5", auto_save=False)
        assert not result.changed
        assert "Invalid spacing value" in result.message
    
    def test_set_line_spacing_inserts_after_begin_document(self, engine):
        """Test that spacing command is inserted after \\begin{document}."""
        result = engine.apply_command("set line spacing 1.5", auto_save=False)
        begin_doc_pos = result.new_text.find("\\begin{document}")
        spacing_pos = result.new_text.find("\\onehalfspacing")
        assert spacing_pos > begin_doc_pos


class TestSetFigureWidth:
    """Test set figure width command."""
    
    def test_set_figure_width(self, engine):
        """Test setting figure width."""
        result = engine.apply_command("set figure width 0.8\\linewidth", auto_save=False)
        assert result.changed
        assert "width=0.8\\linewidth" in result.new_text
        assert "Set includegraphics width to 0.8\\linewidth" in result.message
    
    def test_set_figure_width_all_figures(self, sample_tex):
        """Test that all figures are updated."""
        content = r"""\documentclass{article}
\begin{document}
\includegraphics{fig1.png}
\includegraphics[width=0.5\textwidth]{fig2.png}
\end{document}
"""
        sample_tex.write_text(content, encoding="utf-8")
        engine = LatexEngine(sample_tex)
        result = engine.apply_command("set figure width 0.9\\textwidth", auto_save=False)
        assert result.changed
        assert result.new_text.count("width=0.9\\textwidth") == 2
    
    def test_set_figure_width_no_figures(self, sample_tex):
        """Test behavior when no figures exist."""
        content = r"""\documentclass{article}
\begin{document}
No figures here.
\end{document}
"""
        sample_tex.write_text(content, encoding="utf-8")
        engine = LatexEngine(sample_tex)
        result = engine.apply_command("set figure width 0.8\\textwidth", auto_save=False)
        assert not result.changed
        assert "No includegraphics found" in result.message


class TestChangeDocumentclass:
    """Test change documentclass command."""
    
    def test_change_documentclass(self, engine):
        """Test changing documentclass."""
        result = engine.apply_command("change documentclass to report", auto_save=False)
        assert result.changed
        assert "\\documentclass{report}" in result.new_text
        assert "Changed documentclass to report" in result.message
    
    def test_change_documentclass_preserves_options(self, sample_tex):
        """Test that documentclass options are preserved."""
        content = r"""\documentclass[12pt,a4paper]{article}
\begin{document}
Test
\end{document}
"""
        sample_tex.write_text(content, encoding="utf-8")
        engine = LatexEngine(sample_tex)
        result = engine.apply_command("change documentclass to book", auto_save=False)
        assert result.changed
        assert "\\documentclass[12pt,a4paper]{book}" in result.new_text
    
    def test_change_documentclass_invalid(self, engine):
        """Test validation rejects unknown documentclass."""
        result = engine.apply_command("change documentclass to fakeclass", auto_save=False)
        assert not result.changed
        assert "Invalid documentclass" in result.message


class TestNormalizeWhitespace:
    """Test normalize whitespace command."""
    
    def test_normalize_removes_trailing_spaces(self, sample_tex):
        """Test that trailing spaces are removed."""
        content = "Line with trailing spaces    \nAnother line  \n"
        sample_tex.write_text(content, encoding="utf-8")
        engine = LatexEngine(sample_tex)
        result = engine.apply_command("normalize whitespace", auto_save=False)
        assert result.changed
        assert "spaces    \n" not in result.new_text
        assert "line  \n" not in result.new_text
    
    def test_normalize_collapses_newlines(self, sample_tex):
        """Test that multiple newlines are collapsed."""
        content = "Line 1\n\n\n\nLine 2"
        sample_tex.write_text(content, encoding="utf-8")
        engine = LatexEngine(sample_tex)
        result = engine.apply_command("normalize whitespace", auto_save=False)
        assert result.changed
        assert "\n\n\n" not in result.new_text
        assert "Line 1\n\nLine 2" in result.new_text
    
    def test_normalize_already_clean(self, sample_tex):
        """Test behavior when whitespace is already normalized."""
        content = "Clean content\n\nNo issues\n"
        sample_tex.write_text(content, encoding="utf-8")
        engine = LatexEngine(sample_tex)
        result = engine.apply_command("normalize whitespace", auto_save=False)
        assert not result.changed


class TestAddRemovePackage:
    """Test add and remove package commands."""
    
    def test_add_package_simple(self, engine):
        """Test adding a simple package."""
        result = engine.apply_command("add package hyperref", auto_save=False)
        assert result.changed
        assert "\\usepackage{hyperref}" in result.new_text
        assert "Added package hyperref" in result.message
    
    def test_add_package_with_options(self, engine):
        """Test adding package with options."""
        result = engine.apply_command("add package geometry[margin=1in]", auto_save=False)
        assert result.changed
        assert "\\usepackage[margin=1in]{geometry}" in result.new_text
    
    def test_add_package_already_exists(self, engine):
        """Test that adding existing package is detected."""
        result = engine.apply_command("add package graphicx", auto_save=False)
        assert not result.changed
        assert "already loaded" in result.message
    
    def test_remove_package(self, engine):
        """Test removing a package."""
        result = engine.apply_command("remove package graphicx", auto_save=False)
        assert result.changed
        assert "\\usepackage{graphicx}" not in result.new_text
        assert "Removed package graphicx" in result.message
    
    def test_remove_package_not_found(self, engine):
        """Test removing non-existent package."""
        result = engine.apply_command("remove package fakepackage", auto_save=False)
        assert not result.changed
        assert "not found" in result.message


class TestDiffComputation:
    """Test diff preview functionality."""
    
    def test_compute_diff(self, engine):
        """Test that diff is computed correctly."""
        result = engine.apply_command("set margins 1in", auto_save=False)
        diff = engine.compute_diff(result.new_text)
        assert diff
        assert "+" in diff  # Added lines
        assert "\\usepackage[margin=1in]{geometry}" in diff
    
    def test_compute_diff_no_changes(self, engine):
        """Test diff when no changes are made."""
        diff = engine.compute_diff(engine.text)
        assert diff == ""


class TestBackupSystem:
    """Test backup and save functionality."""
    
    def test_save_creates_timestamped_backup(self, engine, tmp_path):
        """Test that save creates timestamped backup."""
        backup_path = engine.save(backup=True)
        assert backup_path is not None
        assert backup_path.exists()
        assert ".bak." in backup_path.name
        assert backup_path.read_text(encoding="utf-8") == engine.text
    
    def test_save_without_backup(self, engine):
        """Test saving without backup."""
        original_backups = list(engine.path.parent.glob(f"{engine.path.name}.bak.*"))
        backup_path = engine.save(backup=False)
        assert backup_path is None
        new_backups = list(engine.path.parent.glob(f"{engine.path.name}.bak.*"))
        assert len(new_backups) == len(original_backups)
    
    def test_backup_cleanup(self, engine, tmp_path):
        """Test that old backups are cleaned up."""
        engine._max_backups = 3
        
        # Create multiple backups
        for i in range(5):
            engine.save(backup=True)
        
        backups = list(engine.path.parent.glob(f"{engine.path.name}.bak.*"))
        assert len(backups) <= engine._max_backups


class TestUnknownCommand:
    """Test error handling for unknown commands."""
    
    def test_unknown_command(self, engine):
        """Test that unknown commands are handled gracefully."""
        result = engine.apply_command("do something random", auto_save=False)
        assert not result.changed
        assert "Unknown command" in result.message


class TestEdgeCases:
    """Test edge cases and unusual documents."""
    
    def test_no_documentclass(self, sample_tex):
        """Test behavior when documentclass is missing."""
        content = "Just some random text"
        sample_tex.write_text(content, encoding="utf-8")
        engine = LatexEngine(sample_tex)
        result = engine.apply_command("set margins 1in", auto_save=False)
        # Should still insert package, just at start of file
        assert result.changed
        assert "\\usepackage[margin=1in]{geometry}" in result.new_text
    
    def test_no_begin_document(self, sample_tex):
        """Test spacing command when no \\begin{document}."""
        content = r"""\documentclass{article}
Some preamble
"""
        sample_tex.write_text(content, encoding="utf-8")
        engine = LatexEngine(sample_tex)
        result = engine.apply_command("set line spacing 1.5", auto_save=False)
        assert result.changed
        # Should prepend spacing command
        assert "\\onehalfspacing" in result.new_text
