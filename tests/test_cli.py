"""Test CLI functionality."""
import pytest
from pathlib import Path
from unittest.mock import patch, MagicMock
from typer.testing import CliRunner
from latex_ai.cli import app

runner = CliRunner()


@pytest.fixture
def sample_tex_file(tmp_path):
    """Create a sample .tex file for CLI testing."""
    tex_file = tmp_path / "test.tex"
    content = r"""\documentclass{article}

\title{Test}
\author{Author}

\begin{document}
\maketitle
Test content.
\end{document}
"""
    tex_file.write_text(content, encoding="utf-8")
    return tex_file


class TestPreviewCommand:
    """Test the preview CLI command."""
    
    def test_preview_shows_info(self, sample_tex_file):
        """Test that preview displays document information."""
        result = runner.invoke(app, ["preview", str(sample_tex_file)])
        assert result.exit_code == 0
        assert "documentclass" in result.stdout
        assert "article" in result.stdout
    
    def test_preview_nonexistent_file(self):
        """Test preview with non-existent file."""
        result = runner.invoke(app, ["preview", "nonexistent.tex"])
        assert result.exit_code == 1
        assert "not found" in result.stdout.lower()


class TestApplyCommand:
    """Test the apply CLI command."""
    
    def test_apply_modifies_file(self, sample_tex_file):
        """Test that apply command modifies the file."""
        result = runner.invoke(app, ["apply", str(sample_tex_file), "set margins 1in"])
        assert result.exit_code == 0
        assert "Set geometry margin" in result.stdout
        
        # Verify file was modified
        content = sample_tex_file.read_text(encoding="utf-8")
        assert "\\usepackage[margin=1in]{geometry}" in content
    
    def test_apply_dry_run(self, sample_tex_file):
        """Test dry-run mode doesn't modify file."""
        original_content = sample_tex_file.read_text(encoding="utf-8")
        result = runner.invoke(app, ["apply", str(sample_tex_file), "set margins 1in", "--dry-run"])
        assert result.exit_code == 0
        assert "DRY RUN" in result.stdout
        
        # Verify file was NOT modified
        assert sample_tex_file.read_text(encoding="utf-8") == original_content
    
    def test_apply_shows_diff(self, sample_tex_file):
        """Test that apply shows diff preview."""
        result = runner.invoke(app, ["apply", str(sample_tex_file), "set margins 1in"])
        assert "Preview of changes:" in result.stdout
        assert "+" in result.stdout  # Diff markers
    
    def test_apply_creates_backup(self, sample_tex_file):
        """Test that backup is created."""
        result = runner.invoke(app, ["apply", str(sample_tex_file), "set margins 1in"])
        assert result.exit_code == 0
        assert "Backup created" in result.stdout
        
        # Check backup exists
        backups = list(sample_tex_file.parent.glob(f"{sample_tex_file.name}.bak.*"))
        assert len(backups) > 0
    
    def test_apply_invalid_command(self, sample_tex_file):
        """Test handling of invalid commands."""
        result = runner.invoke(app, ["apply", str(sample_tex_file), "do random thing"])
        assert result.exit_code == 0
        assert "Unknown command" in result.stdout


class TestReplCommand:
    """Test the REPL CLI command."""
    
    def test_repl_basic(self, sample_tex_file):
        """Test basic REPL interaction."""
        # Send commands followed by empty line to exit
        result = runner.invoke(app, ["repl", str(sample_tex_file)], input="set margins 1in\n\n")
        assert result.exit_code == 0
        assert "latex-ai>" in result.stdout
        assert "Set geometry margin" in result.stdout
    
    def test_repl_multiple_commands(self, sample_tex_file):
        """Test REPL with multiple sequential commands."""
        commands = "set margins 1in\nset line spacing 1.5\n\n"
        result = runner.invoke(app, ["repl", str(sample_tex_file)], input=commands)
        assert result.exit_code == 0
        assert "Set geometry margin" in result.stdout
        assert "Applied line spacing" in result.stdout
        
        # Verify both changes were applied
        content = sample_tex_file.read_text(encoding="utf-8")
        assert "\\usepackage[margin=1in]{geometry}" in content
        assert "\\onehalfspacing" in content
    
    def test_repl_state_consistency(self, sample_tex_file):
        """Test that REPL maintains state consistency (regression test for state bug)."""
        # This tests that the second command sees changes from the first
        commands = "add package hyperref\nremove package hyperref\n\n"
        result = runner.invoke(app, ["repl", str(sample_tex_file)], input=commands)
        assert result.exit_code == 0
        
        # Final state should have no hyperref package
        content = sample_tex_file.read_text(encoding="utf-8")
        assert "\\usepackage{hyperref}" not in content


class TestPlanCommand:
    """Test the plan CLI command for AI-powered editing."""
    
    def test_plan_missing_api_key(self, sample_tex_file):
        """Test plan command without API key."""
        with patch.dict("os.environ", {}, clear=True):
            result = runner.invoke(app, ["plan", str(sample_tex_file), "Make it pretty"], input="n\n")
            assert result.exit_code != 0
    
    def test_plan_generate_success(self, sample_tex_file):
        """Test successful plan generation."""
        with patch.dict("os.environ", {"GEMINI_API_KEY": "test-key"}):
            with patch("latex_ai.cli.get_ai_planner") as mock_get_planner:
                with patch("latex_ai.cli.typer.confirm", return_value=True):
                    # Create mock plan
                    from latex_ai.ai_planner import EditPlan, EditStep
                    mock_plan = EditPlan(
                        goal="Make conference-ready",
                        summary="Format the document",
                        confidence=0.85
                    )
                    mock_plan.steps = [
                        EditStep("set margins 1in", "Set margins"),
                        EditStep("add package hyperref", "Add hyperref")
                    ]

                    mock_planner = MagicMock()
                    mock_planner.create_plan.return_value = mock_plan
                    mock_get_planner.return_value = mock_planner

                    result = runner.invoke(
                        app,
                        ["plan", str(sample_tex_file), "Make conference-ready"],
                        input="\n"  # Dummy input
                    )
                    assert result.exit_code == 0
                    assert "Make conference-ready" in result.stdout
                    assert "set margins 1in" in result.stdout
                    assert "add package hyperref" in result.stdout
    
    def test_plan_apply_automatically(self, sample_tex_file):
        """Test plan with --apply flag."""
        with patch.dict("os.environ", {"GEMINI_API_KEY": "test-key"}):
            with patch("latex_ai.cli.get_ai_planner") as mock_get_planner:
                with patch("latex_ai.cli.typer.confirm", return_value=True):
                    from latex_ai.ai_planner import EditPlan, EditStep
                    mock_plan = EditPlan(
                        goal="Format",
                        summary="Format document",
                        confidence=0.9
                    )
                    mock_plan.steps = [
                        EditStep("set margins 1in", "Set margins")
                    ]
                    
                    mock_planner = MagicMock()
                    mock_planner.create_plan.return_value = mock_plan
                    mock_get_planner.return_value = mock_planner
                    
                    result = runner.invoke(
                        app,
                        ["plan", str(sample_tex_file), "Format", "--apply"],
                        input="\n"
                    )
                    assert result.exit_code == 0
                    assert "Applying plan" in result.stdout
    
    def test_plan_with_api_error(self, sample_tex_file):
        """Test plan command when AI fails."""
        with patch.dict("os.environ", {"GEMINI_API_KEY": "test-key"}):
            with patch("latex_ai.cli.get_ai_planner") as mock_get_planner:
                with patch("latex_ai.cli.typer.confirm", return_value=True):
                    mock_planner = MagicMock()
                    mock_planner.create_plan.side_effect = ValueError("API failed")
                    mock_get_planner.return_value = mock_planner
                    
                    result = runner.invoke(
                        app,
                        ["plan", str(sample_tex_file), "Test"],
                        input="\n"
                    )
                    assert result.exit_code != 0
                    assert "AI Error" in result.stdout or "API failed" in result.stdout
