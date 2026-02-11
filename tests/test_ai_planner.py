"""Tests for AI planner system."""

import json
import pytest
from pathlib import Path
from unittest.mock import Mock, patch, MagicMock
from latex_ai.ai_planner import AIPlanner, EditPlan, EditStep, get_ai_planner
from latex_ai.document import Document


@pytest.fixture
def mock_document(tmp_path):
    """Create a mock LaTeX document."""
    doc_file = tmp_path / "test.tex"
    doc_file.write_text(r"""\documentclass{article}
\usepackage{amsmath}
\usepackage{graphicx}

\begin{document}

\section{Introduction}
This is a test paper.

\end{document}""")
    return Document(doc_file)


class TestEditStep:
    """Test EditStep dataclass."""
    
    def test_edit_step_creation(self):
        """Test creating an edit step."""
        step = EditStep(
            command="set margins 1in",
            description="Set reasonable margins",
            rationale="Standard conference format"
        )
        assert step.command == "set margins 1in"
        assert step.description == "Set reasonable margins"
        assert step.rationale == "Standard conference format"
    
    def test_edit_step_to_dict(self):
        """Test converting edit step to dict."""
        step = EditStep(
            command="set line spacing 1.5",
            description="Improve readability"
        )
        d = step.to_dict()
        assert d["command"] == "set line spacing 1.5"
        assert d["description"] == "Improve readability"
        assert d["rationale"] == ""


class TestEditPlan:
    """Test EditPlan dataclass."""
    
    def test_edit_plan_creation(self):
        """Test creating an edit plan."""
        plan = EditPlan(goal="Make paper conference-ready")
        assert plan.goal == "Make paper conference-ready"
        assert len(plan.steps) == 0
        assert plan.confidence == 0.8
    
    def test_edit_plan_add_steps(self):
        """Test adding steps to a plan."""
        plan = EditPlan(goal="Format document")
        plan.steps.append(EditStep("set margins 1in", "Set margins"))
        plan.steps.append(EditStep("add package hyperref", "Add hyperref"))
        
        assert len(plan.steps) == 2
        assert plan.steps[0].command == "set margins 1in"
    
    def test_edit_plan_to_dict(self):
        """Test converting plan to dict."""
        plan = EditPlan(
            goal="Format",
            summary="Format the document",
            confidence=0.9
        )
        plan.steps.append(EditStep("set margins 1in", "Set margins"))
        
        d = plan.to_dict()
        assert d["goal"] == "Format"
        assert d["summary"] == "Format the document"
        assert d["confidence"] == 0.9
        assert len(d["steps"]) == 1
    
    def test_edit_plan_to_json(self):
        """Test converting plan to JSON."""
        plan = EditPlan(goal="Format")
        plan.steps.append(EditStep("set margins 1in", "Set margins"))
        
        json_str = plan.to_json()
        parsed = json.loads(json_str)
        assert parsed["goal"] == "Format"
        assert len(parsed["steps"]) == 1
    
    def test_edit_plan_from_dict(self):
        """Test creating plan from dict."""
        data = {
            "goal": "Format",
            "summary": "Format document",
            "confidence": 0.85,
            "steps": [
                {
                    "command": "set margins 1in",
                    "description": "Set margins",
                    "rationale": "Standard"
                }
            ]
        }
        plan = EditPlan.from_dict(data)
        assert plan.goal == "Format"
        assert plan.summary == "Format document"
        assert plan.confidence == 0.85
        assert len(plan.steps) == 1
        assert plan.steps[0].command == "set margins 1in"


class TestAIPlannerInitialization:
    """Test AIPlanner initialization."""
    
    def test_init_with_api_key(self):
        """Test initialization with explicit API key."""
        with patch.dict("os.environ", {}, clear=True):
            with patch("latex_ai.ai_planner.genai"):
                planner = AIPlanner(api_key="test-key-123")
                assert planner.api_key == "test-key-123"
    
    def test_init_from_environment(self):
        """Test initialization from environment variable."""
        with patch.dict("os.environ", {"GEMINI_API_KEY": "env-key"}):
            with patch("latex_ai.ai_planner.genai"):
                planner = AIPlanner()
                assert planner.api_key == "env-key"
    
    def test_init_missing_api_key(self):
        """Test initialization fails without API key."""
        with patch.dict("os.environ", {}, clear=True):
            with pytest.raises(ValueError, match="API key required"):
                AIPlanner()
    
    def test_init_gemini_not_available(self):
        """Test initialization fails if gemini not available."""
        with patch("latex_ai.ai_planner.genai", None):
            with pytest.raises(ValueError, match="google-genai not installed"):
                AIPlanner(api_key="test-key")


class TestAIPlannerContext:
    """Test context preparation."""
    
    def test_prepare_context(self, tmp_path):
        """Test preparing document context."""
        with patch.dict("os.environ", {"GEMINI_API_KEY": "test-key"}):
            with patch("latex_ai.ai_planner.genai"):
                planner = AIPlanner()
                doc_file = tmp_path / "test.tex"
                doc_file.write_text(r"""\documentclass{article}
\usepackage{amsmath}
\usepackage{graphicx}

\begin{document}
Test
\end{document}""")
                doc = Document(doc_file)
                
                context = planner._prepare_context(doc)
                assert "article" in context
                assert "amsmath" in context
                assert "graphicx" in context
                assert "Total lines:" in context


class TestAIPlannerPrompt:
    """Test prompt generation."""
    
    def test_create_prompt(self):
        """Test prompt creation."""
        with patch.dict("os.environ", {"GEMINI_API_KEY": "test-key"}):
            with patch("latex_ai.ai_planner.genai"):
                planner = AIPlanner()
                context = "Test document"
                request = "Make conference-ready"
                
                prompt = planner._create_prompt(context, request)
                assert "LaTeX editor" in prompt
                assert "Test document" in prompt
                assert "Make conference-ready" in prompt
                assert "set margins" in prompt
                assert "add package" in prompt


class TestAIPlannerResponse:
    """Test response parsing."""
    
    def test_parse_valid_json_response(self):
        """Test parsing valid JSON response."""
        with patch.dict("os.environ", {"GEMINI_API_KEY": "test-key"}):
            with patch("latex_ai.ai_planner.genai"):
                planner = AIPlanner()
                
                response_text = """{
                    "goal": "Format",
                    "summary": "Make it pretty",
                    "confidence": 0.9,
                    "steps": [
                        {
                            "command": "set margins 1in",
                            "description": "Set margins",
                            "rationale": "Standard"
                        }
                    ]
                }"""
                
                result = planner._parse_response(response_text)
                assert result["goal"] == "Format"
                assert len(result["steps"]) == 1
    
    def test_parse_json_with_extra_text(self):
        """Test parsing JSON embedded in text."""
        with patch.dict("os.environ", {"GEMINI_API_KEY": "test-key"}):
            with patch("latex_ai.ai_planner.genai"):
                planner = AIPlanner()
                
                response_text = """Here's the plan:
                
                {
                    "goal": "Format",
                    "summary": "Make it pretty",
                    "confidence": 0.9,
                    "steps": []
                }
                
                That's the plan!"""
                
                result = planner._parse_response(response_text)
                assert result["goal"] == "Format"
    
    def test_parse_invalid_json(self):
        """Test parsing invalid JSON."""
        with patch.dict("os.environ", {"GEMINI_API_KEY": "test-key"}):
            with patch("latex_ai.ai_planner.genai"):
                planner = AIPlanner()
                
                # This will be detected as "No JSON found" since regex won't match
                with pytest.raises(ValueError, match="No JSON found|Invalid JSON"):
                    planner._parse_response("{ invalid json")
    
    def test_parse_no_json(self):
        """Test parsing response without JSON."""
        with patch.dict("os.environ", {"GEMINI_API_KEY": "test-key"}):
            with patch("latex_ai.ai_planner.genai"):
                planner = AIPlanner()
                
                with pytest.raises(ValueError, match="No JSON found"):
                    planner._parse_response("No JSON here")


class TestAIPlannerCreatePlan:
    """Test plan creation."""
    
    def test_create_plan_success(self, mock_document):
        """Test successful plan creation."""
        with patch.dict("os.environ", {"GEMINI_API_KEY": "test-key"}):
            with patch("latex_ai.ai_planner.genai") as mock_genai:
                # Mock the client and response
                mock_response = MagicMock()
                mock_response.text = """{
                    "goal": "Make conference-ready",
                    "summary": "Format document",
                    "confidence": 0.85,
                    "steps": [
                        {
                            "command": "set margins 1in",
                            "description": "Set margins",
                            "rationale": "Conference standard"
                        },
                        {
                            "command": "add package hyperref",
                            "description": "Add hyperref",
                            "rationale": "Better references"
                        }
                    ]
                }"""
                
                mock_client = MagicMock()
                mock_client.models.generate_content.return_value = mock_response
                mock_genai.Client.return_value = mock_client
                
                planner = AIPlanner()
                plan = planner.create_plan(mock_document, "Make conference-ready")
                
                assert plan.goal == "Make conference-ready"
                assert len(plan.steps) == 2
                assert plan.steps[0].command == "set margins 1in"
                assert plan.confidence == 0.85
    
    def test_create_plan_api_error(self, mock_document):
        """Test plan creation with API error."""
        with patch.dict("os.environ", {"GEMINI_API_KEY": "test-key"}):
            with patch("latex_ai.ai_planner.genai") as mock_genai:
                mock_client = MagicMock()
                mock_client.models.generate_content.side_effect = Exception("API error")
                mock_genai.Client.return_value = mock_client
                
                planner = AIPlanner()
                with pytest.raises(ValueError, match="Failed to generate plan"):
                    planner.create_plan(mock_document, "Test request")


class TestGlobalAIPlanner:
    """Test global AI planner singleton."""
    
    def test_get_planner_singleton(self):
        """Test getting global planner returns same instance."""
        with patch.dict("os.environ", {"GEMINI_API_KEY": "test-key"}):
            with patch("latex_ai.ai_planner.genai"):
                # Reset singleton
                from latex_ai.ai_planner import GlobalAIPlanner
                GlobalAIPlanner._instance = None
                
                planner1 = get_ai_planner()
                planner2 = get_ai_planner()
                
                assert planner1 is planner2
    
    def test_get_planner_with_api_key(self):
        """Test getting planner with explicit API key."""
        with patch.dict("os.environ", {}, clear=True):
            with patch("latex_ai.ai_planner.genai"):
                from latex_ai.ai_planner import GlobalAIPlanner
                GlobalAIPlanner._instance = None
                
                planner = get_ai_planner(api_key="test-key")
                assert planner.api_key == "test-key"
