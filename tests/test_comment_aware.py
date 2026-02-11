"""Tests for comment-aware editing behavior."""
import pytest
from pathlib import Path
from latex_ai.document import Document
from latex_ai.engine import LatexEngine


@pytest.fixture
def doc_with_comments(tmp_path):
    """Create document with commented sections."""
    tex_file = tmp_path / "commented.tex"
    content = r"""\documentclass{article}
% \usepackage{tikz}  % This package is commented out
\usepackage{graphicx}
% \usepackage{hyperref}

\begin{document}
% \begin{figure}  % Commented environment
Hello world!
\end{document}
"""
    tex_file.write_text(content, encoding="utf-8")
    return tex_file


@pytest.fixture
def engine_with_comments(doc_with_comments):
    """Create engine with commented document."""
    return LatexEngine(doc_with_comments)


class TestCommentAwarePackageDetection:
    """Test that package detection ignores comments."""
    
    def test_get_packages_ignores_comments(self, doc_with_comments):
        """Test that commented packages are not detected."""
        doc = Document(doc_with_comments)
        packages = doc.get_packages()
        
        pkg_names = [pkg for pkg, _ in packages]
        assert "graphicx" in pkg_names
        assert "tikz" not in pkg_names  # Commented out
        assert "hyperref" not in pkg_names  # Commented out
    
    def test_has_package_respects_comments(self, doc_with_comments):
        """Test that has_package ignores commented packages."""
        doc = Document(doc_with_comments)
        
        assert doc.has_package("graphicx")
        assert not doc.has_package("tikz")  # Commented
        assert not doc.has_package("hyperref")  # Commented


class TestCommentAwareEditing:
    """Test that editing operations don't modify comments."""
    
    def test_add_package_doesnt_uncomment(self, engine_with_comments):
        """Test adding package doesn't uncomment existing package."""
        result = engine_with_comments.apply_command("add package amsmath")
        
        assert result.changed
        # Original comments should still be there
        assert "% \\usepackage{tikz}" in result.new_text
        assert "% \\usepackage{hyperref}" in result.new_text
        # New package should be added
        assert "\\usepackage{amsmath}" in result.new_text
    
    def test_remove_package_only_active(self, engine_with_comments):
        """Test removing package only removes active (non-commented) one."""
        # Try to remove tikz (which is commented)
        result = engine_with_comments.apply_command("remove package tikz")
        
        # Should not change anything (tikz is already commented)
        assert not result.changed or "not found" in result.message.lower()
    
    def test_set_margins_respects_comments(self, tmp_path):
        """Test that margin setting doesn't modify commented geometry."""
        tex_file = tmp_path / "test.tex"
        content = r"""\documentclass{article}
% \usepackage[margin=2in]{geometry}  % Old setting
\usepackage{graphicx}

\begin{document}
Hello!
\end{document}
"""
        tex_file.write_text(content, encoding="utf-8")
        engine = LatexEngine(tex_file)
        
        result = engine.apply_command("set margins 1in")
        
        assert result.changed
        # Comment line should still exist (comment preserved)
        assert "% " in result.new_text and "geometry" in result.new_text
        # New geometry package should be added
        assert "margin=1in" in result.new_text


class TestCommentAwareEnvironmentDetection:
    """Test environment detection with comments."""
    
    def test_find_environment_ignores_commented(self, doc_with_comments):
        """Test that commented environments are ignored."""
        doc = Document(doc_with_comments)
        
        # document environment should be found (not commented)
        doc_env = doc.find_environment("document")
        assert doc_env is not None
        
        # figure environment should NOT be found (commented out)
        fig_env = doc.find_environment("figure")
        assert fig_env is None


class TestInlineComments:
    """Test handling of inline comments."""
    
    def test_inline_comment_after_package(self, tmp_path):
        """Test package with inline comment."""
        tex_file = tmp_path / "inline.tex"
        content = r"""\documentclass{article}
\usepackage{graphicx} % For images
\usepackage{amsmath}  % For math

\begin{document}
Hello!
\end{document}
"""
        tex_file.write_text(content, encoding="utf-8")
        doc = Document(tex_file)
        
        packages = doc.get_packages()
        pkg_names = [pkg for pkg, _ in packages]
        
        # Both packages should be detected
        assert "graphicx" in pkg_names
        assert "amsmath" in pkg_names
    
    def test_add_package_preserves_inline_comments(self, tmp_path):
        """Test that adding package preserves existing inline comments."""
        tex_file = tmp_path / "inline.tex"
        content = r"""\documentclass{article}
\usepackage{graphicx} % For images

\begin{document}
Hello!
\end{document}
"""
        tex_file.write_text(content, encoding="utf-8")
        engine = LatexEngine(tex_file)
        
        result = engine.apply_command("add package amsmath")
        
        assert result.changed
        # Inline comment should still be there
        assert "% For images" in result.new_text


class TestCommentedDocumentclass:
    """Test handling of commented documentclass."""
    
    def test_find_documentclass_ignores_comment(self, tmp_path):
        """Test that commented documentclass is ignored."""
        tex_file = tmp_path / "commented_class.tex"
        content = r"""% \documentclass{report}  % Old class
\documentclass{article}

\begin{document}
Hello!
\end{document}
"""
        tex_file.write_text(content, encoding="utf-8")
        doc = Document(tex_file)
        
        klass, opts = doc.find_documentclass()
        assert klass == "article"  # Not "report"


class TestPartiallyCommentedLines:
    """Test handling of partially commented lines."""
    
    def test_package_before_comment(self, tmp_path):
        """Test package declaration before comment on same line."""
        tex_file = tmp_path / "partial.tex"
        content = r"""\documentclass{article}
\usepackage{graphicx}  % This is a comment
% The above line should work

\begin{document}
Hello!
\end{document}
"""
        tex_file.write_text(content, encoding="utf-8")
        doc = Document(tex_file)
        
        assert doc.has_package("graphicx")
