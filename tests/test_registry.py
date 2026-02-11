"""Tests for the command registry system and Document abstraction."""
import pytest
from pathlib import Path
from latex_ai.commands import Command, CommandRegistry
from latex_ai.document import Document
from latex_ai.types import EditResult


@pytest.fixture
def sample_tex_file(tmp_path):
    """Create a sample .tex file for testing."""
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
def document(sample_tex_file):
    """Create a Document instance."""
    return Document(sample_tex_file)


class TestCommandRegistry:
    """Test the command registry system."""
    
    def test_registry_has_commands_registered(self):
        """Test that builtin commands are registered."""
        commands = CommandRegistry.list_commands()
        assert len(commands) >= 7, "At least 7 builtin commands should be registered"
    
    def test_registry_find_command(self):
        """Test finding a command by text."""
        result = CommandRegistry.find_command("set margins 1in")
        assert result is not None, "Should find 'set margins' command"
        command_class, args = result
        assert args["size"] == "1in"
    
    def test_registry_get_command_by_name(self):
        """Test getting command by name."""
        cmd = CommandRegistry.get_command("set_margins")
        assert cmd is not None
        assert cmd.name == "set_margins"
    
    def test_registry_list_commands_has_metadata(self):
        """Test that listed commands have proper metadata."""
        commands = CommandRegistry.list_commands()
        for cmd in commands:
            assert cmd.name
            assert cmd.description
            assert cmd.examples
            assert cmd.handler_class


class TestDocumentAPIs:
    """Test the Document abstraction layer."""
    
    def test_document_initialization(self, document):
        """Test Document loads file correctly."""
        assert document.path
        assert "documentclass{article}" in document.text
    
    def test_find_documentclass(self, document):
        """Test extracting documentclass."""
        klass, opts = document.find_documentclass()
        assert klass == "article"
        assert opts is None
    
    def test_find_documentclass_with_options(self, sample_tex_file):
        """Test extracting documentclass with options."""
        content = r"\documentclass[12pt,a4paper]{article}" + "\n"
        sample_tex_file.write_text(content, encoding="utf-8")
        doc = Document(sample_tex_file)
        klass, opts = doc.find_documentclass()
        assert klass == "article"
        assert "12pt" in opts
    
    def test_find_preamble_end(self, document):
        """Test finding end of preamble."""
        preamble_end = document.find_preamble_end()
        assert preamble_end > 0
        assert "\\begin{document}" in document.text[preamble_end:]
    
    def test_get_packages(self, document):
        """Test extracting loaded packages."""
        packages = document.get_packages()
        assert any(pkg == "graphicx" for pkg, _ in packages)
    
    def test_has_package(self, document):
        """Test checking if package is loaded."""
        assert document.has_package("graphicx")
        assert not document.has_package("hyperref")
    
    def test_insert_preamble(self, document):
        """Test inserting content into preamble."""
        new_text = document.insert_preamble("\\usepackage{tikz}")
        assert "\\usepackage{tikz}" in new_text
        # Should be after documentclass
        docclass_pos = new_text.find("\\documentclass")
        tikz_pos = new_text.find("\\usepackage{tikz}")
        assert docclass_pos < tikz_pos
    
    def test_insert_after_begin_document(self, document):
        """Test inserting after begin{document}."""
        new_text = document.insert_after_begin_document("\\maketitle")
        begin_doc_pos = new_text.find("\\begin{document}")
        maketitle_pos = new_text.find("\\maketitle")
        assert begin_doc_pos < maketitle_pos
    
    def test_replace_documentclass(self, document):
        """Test replacing documentclass."""
        new_text = document.replace_documentclass("report")
        assert "\\documentclass{report}" in new_text
        assert "\\documentclass{article}" not in new_text
    
    def test_add_package(self, document):
        """Test adding a new package."""
        new_text = document.add_package("hyperref")
        assert "\\usepackage{hyperref}" in new_text
    
    def test_add_package_with_options(self, document):
        """Test adding package with options."""
        new_text = document.add_package("geometry", "margin=1in")
        assert "\\usepackage[margin=1in]{geometry}" in new_text
    
    def test_remove_package(self, document):
        """Test removing a package."""
        new_text = document.remove_package("graphicx")
        assert "\\usepackage{graphicx}" not in new_text
    
    def test_find_environment(self, document):
        """Test finding a LaTeX environment."""
        env_range = document.find_environment("figure")
        assert env_range is not None
        assert "\\includegraphics" in document.text[env_range.start.offset:env_range.end.offset]


class TestCommandRegistryPatternMatching:
    """Test pattern matching in command registration."""
    
    def test_set_margins_pattern(self):
        """Test set margins command pattern."""
        result = CommandRegistry.find_command("set margins 2.5cm")
        assert result is not None
        cmd_class, args = result
        assert args["size"] == "2.5cm"
    
    def test_set_line_spacing_pattern(self):
        """Test set line spacing pattern."""
        result = CommandRegistry.find_command("set line spacing 1.5")
        assert result is not None
        cmd_class, args = result
        assert args["value"] == "1.5"
    
    def test_add_package_pattern(self):
        """Test add package pattern."""
        result = CommandRegistry.find_command("add package tikz")
        assert result is not None
        cmd_class, args = result
        assert args["package"] == "tikz"
    
    def test_pattern_case_insensitive(self):
        """Test patterns are case-insensitive."""
        result = CommandRegistry.find_command("SET MARGINS 1in")
        assert result is not None
    
    def test_invalid_command_not_matched(self):
        """Test that invalid commands don't match."""
        result = CommandRegistry.find_command("do something random")
        assert result is None


class TestCommandExecution:
    """Test executing commands from the registry."""
    
    def test_execute_set_margins_command(self, document):
        """Test executing set margins through registry."""
        result = CommandRegistry.find_command("set margins 1in")
        assert result is not None
        cmd_class, args = result
        cmd = cmd_class(document)
        edit_result = cmd.execute(**args)
        assert edit_result.changed
        assert "margin=1in" in edit_result.new_text
    
    def test_execute_add_package_command(self, document):
        """Test executing add package through registry."""
        result = CommandRegistry.find_command("add package hyperref")
        assert result is not None
        cmd_class, args = result
        cmd = cmd_class(document)
        edit_result = cmd.execute(**args)
        assert edit_result.changed
        assert "\\usepackage{hyperref}" in edit_result.new_text
    
    def test_validation_catches_invalid_input(self, document):
        """Test that validation works."""
        result = CommandRegistry.find_command("set margins banana")
        if result:
            cmd_class, args = result
            cmd = cmd_class(document)
            edit_result = cmd.execute(**args)
            assert not edit_result.changed
            assert "Invalid" in edit_result.message
