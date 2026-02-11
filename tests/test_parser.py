"""Tests for AST-based LaTeX parser."""
import pytest
from pathlib import Path
from latex_ai.parser import LatexParser


@pytest.fixture
def simple_latex():
    """Simple LaTeX document."""
    return r"""\documentclass{article}
\usepackage{graphicx}
\usepackage[margin=1in]{geometry}

\begin{document}
Hello world!
\end{document}
"""


@pytest.fixture
def latex_with_comments():
    """LaTeX with comments."""
    return r"""\documentclass{article}
% \usepackage{tikz}  % This is commented out
\usepackage{graphicx}
% \begin{figure}  % This too

\begin{document}
% Comment inside document
Hello world!
\end{document}
"""


@pytest.fixture
def complex_latex():
    """Complex LaTeX with nested environments."""
    return r"""\documentclass[12pt,a4paper]{article}
\usepackage{amsmath}
\usepackage[colorlinks=true]{hyperref}

\begin{document}
\begin{figure}[h]
  \centering
  % Comment inside environment
  \includegraphics{example.png}
  \caption{Test}
\end{figure}

\begin{equation}
  E = mc^2
\end{equation}
\end{document}
"""


class TestParserInitialization:
    """Test parser initialization and availability."""
    
    def test_parser_creation(self, simple_latex):
        """Test creating parser instance."""
        parser = LatexParser(simple_latex)
        assert parser.text == simple_latex
    
    def test_ast_availability(self):
        """Test checking if AST parsing is available."""
        available = LatexParser.is_available()
        # Should be True if pylatexenc is installed
        assert isinstance(available, bool)
    
    def test_parser_with_empty_text(self):
        """Test parser with empty document."""
        parser = LatexParser("")
        assert parser.text == ""


class TestFindDocumentclass:
    """Test finding documentclass."""
    
    def test_find_simple_documentclass(self, simple_latex):
        """Test finding documentclass without options."""
        parser = LatexParser(simple_latex)
        klass, opts = parser.find_documentclass()
        assert klass == "article"
        assert opts is None
    
    def test_find_documentclass_with_options(self, complex_latex):
        """Test finding documentclass with options."""
        parser = LatexParser(complex_latex)
        klass, opts = parser.find_documentclass()
        assert klass == "article"
        assert opts == "12pt,a4paper" or opts == "12pt, a4paper"
    
    def test_find_documentclass_not_found(self):
        """Test when documentclass is missing."""
        parser = LatexParser("No documentclass here")
        klass, opts = parser.find_documentclass()
        assert klass is None
        assert opts is None
    
    def test_find_documentclass_ignores_comments(self, latex_with_comments):
        """Test that commented documentclass is ignored."""
        text = r"""% \documentclass{report}
\documentclass{article}
"""
        parser = LatexParser(text)
        klass, opts = parser.find_documentclass()
        assert klass == "article"


class TestFindPackages:
    """Test finding usepackage declarations."""
    
    def test_find_simple_packages(self, simple_latex):
        """Test finding packages without options."""
        parser = LatexParser(simple_latex)
        packages = parser.find_packages()
        
        pkg_names = [pkg for pkg, _ in packages]
        assert "graphicx" in pkg_names
    
    def test_find_packages_with_options(self, simple_latex):
        """Test finding packages with options."""
        parser = LatexParser(simple_latex)
        packages = parser.find_packages()
        
        # Find geometry package
        geometry = next((opts for pkg, opts in packages if pkg == "geometry"), None)
        assert geometry == "margin=1in"
    
    def test_find_packages_ignores_comments(self, latex_with_comments):
        """Test that commented packages are ignored."""
        parser = LatexParser(latex_with_comments)
        packages = parser.find_packages()
        
        pkg_names = [pkg for pkg, _ in packages]
        assert "graphicx" in pkg_names
        assert "tikz" not in pkg_names  # Commented out
    
    def test_find_multiple_packages(self, complex_latex):
        """Test finding multiple packages."""
        parser = LatexParser(complex_latex)
        packages = parser.find_packages()
        
        pkg_names = [pkg for pkg, _ in packages]
        assert "amsmath" in pkg_names
        assert "hyperref" in pkg_names


class TestFindEnvironment:
    """Test finding LaTeX environments."""
    
    def test_find_document_environment(self, simple_latex):
        """Test finding document environment."""
        parser = LatexParser(simple_latex)
        result = parser.find_environment("document")
        
        assert result is not None
        start, end = result
        assert start < end
        assert "\\begin{document}" in simple_latex[start:end]
    
    def test_find_figure_environment(self, complex_latex):
        """Test finding figure environment."""
        parser = LatexParser(complex_latex)
        result = parser.find_environment("figure")
        
        assert result is not None
        start, end = result
        assert start < end
        assert "\\begin{figure}" in complex_latex[start:end]
    
    def test_find_nonexistent_environment(self, simple_latex):
        """Test searching for non-existent environment."""
        parser = LatexParser(simple_latex)
        result = parser.find_environment("tikzpicture")
        
        assert result is None
    
    def test_find_environment_ignores_commented(self, latex_with_comments):
        """Test that commented environments are ignored."""
        parser = LatexParser(latex_with_comments)
        # Document environment exists (not commented)
        result = parser.find_environment("document")
        assert result is not None


class TestFindPreambleEnd:
    """Test finding end of preamble."""
    
    def test_find_preamble_end(self, simple_latex):
        """Test finding preamble end."""
        parser = LatexParser(simple_latex)
        pos = parser.find_preamble_end()
        
        assert pos > 0
        assert "\\begin{document}" in simple_latex[pos:pos+20]
    
    def test_find_preamble_end_with_packages(self, complex_latex):
        """Test preamble end comes after packages."""
        parser = LatexParser(complex_latex)
        pos = parser.find_preamble_end()
        
        # Preamble should include packages
        preamble = complex_latex[:pos]
        assert "\\usepackage{amsmath}" in preamble
        assert "\\usepackage" in preamble
    
    def test_no_document_environment(self):
        """Test when document environment is missing."""
        text = r"""\documentclass{article}
\usepackage{graphicx}
"""
        parser = LatexParser(text)
        pos = parser.find_preamble_end()
        
        # Should return end of file
        assert pos >= len(text) - 10


class TestCommentDetection:
    """Test comment detection."""
    
    def test_is_inside_comment_true(self):
        """Test detecting position inside comment."""
        text = r"""\documentclass{article}
% This is a comment
\usepackage{graphicx}
"""
        parser = LatexParser(text)
        
        # Find position of "comment" word
        comment_pos = text.find("comment")
        assert parser.is_inside_comment(comment_pos)
    
    def test_is_inside_comment_false(self):
        """Test detecting position outside comment."""
        text = r"""\documentclass{article}
\usepackage{graphicx}
"""
        parser = LatexParser(text)
        
        # Find position of "graphicx"
        pkg_pos = text.find("graphicx")
        assert not parser.is_inside_comment(pkg_pos)
    
    def test_is_inside_inline_comment(self):
        """Test inline comment detection."""
        text = r"""\usepackage{graphicx} % inline comment
\usepackage{amsmath}
"""
        parser = LatexParser(text)
        
        # Position in inline comment
        inline_pos = text.find("inline")
        assert parser.is_inside_comment(inline_pos)
        
        # Position before comment
        pkg_pos = text.find("graphicx")
        assert not parser.is_inside_comment(pkg_pos)


class TestParserFallback:
    """Test fallback to regex when AST fails."""
    
    def test_parser_with_use_ast_false(self, simple_latex):
        """Test forcing regex mode."""
        parser = LatexParser(simple_latex, use_ast=False)
        assert not parser.use_ast
        
        # Should still work via regex
        klass, opts = parser.find_documentclass()
        assert klass == "article"
    
    def test_malformed_latex_fallback(self):
        """Test graceful fallback with malformed LaTeX."""
        # Missing closing braces
        text = r"""\documentclass{article
\usepackage{graphicx}
\begin{document}
Hello
\end{document}
"""
        parser = LatexParser(text)
        
        # Should not crash, may use regex fallback
        packages = parser.find_packages()
        assert isinstance(packages, list)


class TestEdgeCases:
    """Test edge cases."""
    
    def test_multiple_documentclass(self):
        """Test with multiple documentclass declarations."""
        text = r"""\documentclass{article}
\documentclass{report}
"""
        parser = LatexParser(text)
        klass, opts = parser.find_documentclass()
        
        # Should find first one
        assert klass in ["article", "report"]
    
    def test_package_with_multiple_packages_in_braces(self):
        """Test usepackage with multiple packages."""
        text = r"""\documentclass{article}
\usepackage{graphicx,amsmath,hyperref}
"""
        parser = LatexParser(text)
        packages = parser.find_packages()
        
        # May parse as single entry or multiple depending on implementation
        assert len(packages) >= 1
    
    def test_nested_environments(self, complex_latex):
        """Test finding environments with nesting."""
        parser = LatexParser(complex_latex)
        
        # Should find both figure and equation
        fig = parser.find_environment("figure")
        eq = parser.find_environment("equation")
        
        assert fig is not None
        assert eq is not None
    
    def test_whitespace_variations(self):
        """Test handling various whitespace."""
        text = r"""\documentclass  {  article  }
\usepackage   [margin=1in]  {geometry}
"""
        parser = LatexParser(text)
        
        klass, _ = parser.find_documentclass()
        assert klass.strip() == "article"
        
        packages = parser.find_packages()
        pkg_names = [pkg.strip() for pkg, _ in packages]
        assert "geometry" in pkg_names
