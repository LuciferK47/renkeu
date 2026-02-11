"""LaTeX AI prompt templates for Gemini API."""


def create_plan_prompt(context: str, request: str) -> str:
    """Create prompt for Gemini to generate an editing plan.

    Args:
        context: Document context string
        request: User's natural language request

    Returns:
        Prompt string for model
    """
    return f"""You are an expert LaTeX editor. Analyze the document and create a step-by-step editing plan.

{context}

User Request: {request}

Generate a JSON response with this structure:
{{
  "goal": "Summary of the request",
  "summary": "Overall strategy",
  "confidence": 0.8,
  "steps": [
    {{
      "command": "set margins 1in",
      "description": "Set reasonable margins",
      "rationale": "Conference papers typically use 1-inch margins"
    }},
    {{
      "command": "add package hyperref",
      "description": "Add hyperref package for clickable references",
      "rationale": "Recommended for conference submissions"
    }}
  ]
}}

Important:
- Only suggest commands from: set margins, set line spacing, add package, remove package, set figure width, change documentclass, normalize whitespace
- Each command should be realistic and valid for the document
- Provide 2-5 steps max
- Return ONLY valid JSON, no other text

Generate the plan now:"""


def create_error_prompt(context: str, diagnostics: list[dict]) -> str:
    """Create a prompt focused on explaining LaTeX compilation errors and proposing fixes.

    Args:
        context: Focused document context
        diagnostics: List of error/warning dicts: {file, line, message, type}

    Returns:
        Prompt string for model
    """
    issues_text = "\n".join([
        f"- [{d.get('type','error')}] {d.get('file','current file')}:{d.get('line','?')} - {d.get('message','')}"
        for d in diagnostics[:50]
    ])

    return f"""You are an expert LaTeX engineer. Explain the following compilation issues and propose concrete fixes.

{context}

Compilation Issues:
{issues_text}

Return ONLY valid JSON with this structure:
{{
  "explanation": "Human-readable explanation of what went wrong overall",
  "likely_causes": ["cause 1", "cause 2"],
  "suggested_fixes": ["high-level fix 1", "high-level fix 2"],
  "patch_steps": [
    {{
      "command": "add package amsmath",
      "description": "Add math package to resolve missing environment",
      "rationale": "Errors indicate missing align environment which needs amsmath"
    }}
  ]
}}

Important:
- Focus on LaTeX correctness.
- Where a package is missing, propose adding it.
- If an environment or command is wrong, propose the correct one.
- Keep patch_steps to 1-5 items maximum.
- Return ONLY JSON, no prose.
"""


def create_refinement_prompt(context: str, refinement_request: str) -> str:
    """Create prompt for document refinement (formatting, styling, structure).

    Args:
        context: Document context
        refinement_request: Natural language refinement request (e.g., "increase font size to 12pt")

    Returns:
        Prompt string for model
    """
    return f"""You are an expert LaTeX document formatter and stylist. The user requests a refinement to their document.

Current document:
{context}

Refinement request: {refinement_request}

Generate a step-by-step plan to refine the document. Return ONLY valid JSON:
{{
  "summary": "Brief description of what will be changed",
  "changes": [
    {{
      "location": "Where in the document (e.g., 'preamble', 'document class line', 'all text commands')",
      "action": "What to change (e.g., 'replace \\\\normalsize with \\\\large')",
      "rationale": "Why this improves the document"
    }}
  ],
  "patch_steps": [
    {{
      "command": "LaTeX command or action (e.g., 'add \\\\usepackage{{geometry}} with margins=1in')",
      "description": "What this command does",
      "rationale": "Why it achieves the refinement"
    }}
  ]
}}

Important:
- Focus on LaTeX packages and commands that achieve the refinement request.
- If a package is needed (e.g., xcolor for colors), suggest adding it.
- Keep changes minimal and targeted.
- Return ONLY JSON, no prose.
"""


def create_suggestions_prompt(context: str) -> str:
    """Create prompt for proactive document improvement suggestions.

    Args:
        context: Document context

    Returns:
        Prompt string for model
    """
    return f"""You are an expert LaTeX document reviewer. Analyze this LaTeX document and suggest 3-5 improvements.

Document:
{context}

Generate improvement suggestions. Return ONLY valid JSON:
{{
  "overall_assessment": "Brief assessment of document quality and readability",
  "strengths": ["strength 1", "strength 2"],
  "suggestions": [
    {{
      "category": "Package/Import|Structure|Formatting|Style",
      "issue": "What could be improved",
      "suggestion": "Specific suggestion with LaTeX code or commands",
      "impact": "How this improves the document (e.g., 'readability', 'compatibility', 'performance')",
      "priority": "high|medium|low"
    }}
  ]
}}

Important:
- Focus on common LaTeX best practices and improvements.
- Suggest missing packages that would help (e.g., hyperref, babel, graphicx).
- Identify structural issues (missing sections, metadata, etc.).
- Provide concrete LaTeX commands or suggestions.
- Prioritize high-impact improvements.
- Return ONLY JSON, no prose.
"""


def create_review_prompt(context: str) -> str:
    """Create prompt for comprehensive document review.

    Args:
        context: Document context

    Returns:
        Prompt string for model
    """
    return f"""You are an expert LaTeX code reviewer. Perform a comprehensive review of this LaTeX document.

Document:
{context}

Analyze the document thoroughly and provide a detailed code review. Return ONLY valid JSON:
{{
  "review_score": 85,
  "executive_summary": "Brief overview of the document quality and key findings",
  "structure_analysis": {{
    "rating": "Excellent|Good|Fair|Needs Work",
    "findings": ["finding 1", "finding 2"],
    "recommendations": ["recommendation 1", "recommendation 2"]
  }},
  "package_analysis": {{
    "rating": "Excellent|Good|Fair|Needs Work",
    "missing_packages": ["package1", "package2"],
    "unused_packages": ["package3"],
    "recommendations": ["recommendation 1"]
  }},
  "style_analysis": {{
    "rating": "Excellent|Good|Fair|Needs Work",
    "issues": ["issue 1", "issue 2"],
    "recommendations": ["recommendation 1", "recommendation 2"]
  }},
  "best_practices": {{
    "rating": "Excellent|Good|Fair|Needs Work",
    "violations": ["violation 1", "violation 2"],
    "recommendations": ["recommendation 1", "recommendation 2"]
  }},
  "priority_actions": [
    {{
      "priority": "critical|high|medium|low",
      "action": "What to do",
      "reason": "Why it matters",
      "code": "LaTeX command or snippet (optional)"
    }}
  ]
}}

Focus on:
1. Document structure (sections, hierarchy, organization)
2. Package usage (missing, redundant, best choices)
3. LaTeX style and formatting consistency
4. Best practices (metadata, references, labels)
5. Actionable improvements with specific code suggestions

Return ONLY JSON, no prose.
"""
