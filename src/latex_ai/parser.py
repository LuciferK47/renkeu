"""AST-based LaTeX parser using pylatexenc.

This module provides robust LaTeX parsing that handles:
- Comments (% lines)
- Nested environments
- Custom macros
- Malformed LaTeX (graceful degradation)
"""
from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path
from typing import Optional, List, Tuple

try:
    from pylatexenc.latexwalker import (
        LatexWalker,
        LatexEnvironmentNode,
        LatexMacroNode,
        LatexCommentNode,
        LatexNode,
        LatexWalkerParseError,
    )
    PYLATEXENC_AVAILABLE = True
except ImportError:
    PYLATEXENC_AVAILABLE = False


@dataclass
class ParsedNode:
    """Represents a parsed LaTeX node with position information."""
    type: str  # 'macro', 'environment', 'comment', 'text'
    name: Optional[str]  # Name of macro/environment
    content: str  # Raw content
    start: int  # Start position in text
    end: int  # End position in text
    args: Optional[List[str]] = None  # Macro/environment arguments
    
    def __repr__(self):
        return f"ParsedNode({self.type}, {self.name}, pos={self.start}-{self.end})"


class LatexParser:
    """AST-based LaTeX parser with fallback to regex."""
    
    def __init__(self, text: str, use_ast: bool = True):
        """Initialize parser.
        
        Args:
            text: LaTeX source text
            use_ast: Use AST parsing if pylatexenc available
        """
        self.text = text
        self.use_ast = use_ast and PYLATEXENC_AVAILABLE
        self._walker: Optional[LatexWalker] = None
        self._nodes: Optional[List[LatexNode]] = None
        
        if self.use_ast:
            try:
                self._parse_ast()
            except Exception:
                # Graceful fallback to regex
                self.use_ast = False
    
    def _parse_ast(self):
        """Parse text using pylatexenc AST."""
        self._walker = LatexWalker(self.text)
        try:
            self._nodes, _, _ = self._walker.get_latex_nodes()
        except LatexWalkerParseError:
            # Malformed LaTeX - fallback to regex
            self.use_ast = False
            self._nodes = None
    
    def find_documentclass(self) -> Tuple[Optional[str], Optional[str]]:
        """Find documentclass and options.
        
        Returns:
            Tuple of (class_name, options_string) or (None, None)
        """
        if self.use_ast and self._nodes:
            return self._find_documentclass_ast()
        return self._find_documentclass_regex()
    
    def _find_documentclass_ast(self) -> Tuple[Optional[str], Optional[str]]:
        """Find documentclass using AST traversal."""
        for node in self._traverse_nodes(self._nodes):
            if isinstance(node, LatexMacroNode) and node.macroname == "documentclass":
                # Extract arguments
                args = self._extract_macro_args(node)
                if args:
                    # documentclass can have optional [options] and required {class}
                    if len(args) >= 2:
                        return args[1], args[0]  # (class, options)
                    elif len(args) == 1:
                        return args[0], None
        return None, None
    
    def _find_documentclass_regex(self) -> Tuple[Optional[str], Optional[str]]:
        """Find documentclass using regex (fallback)."""
        # Skip comments
        lines = [line for line in self.text.split('\n') if not line.strip().startswith('%')]
        text_no_comments = '\n'.join(lines)
        
        match = re.search(
            r'\\documentclass\s*(?:\[([^\]]+)\])?\s*\{([^}]+)\}',
            text_no_comments
        )
        if match:
            return match.group(2), match.group(1)
        return None, None
    
    def find_packages(self) -> List[Tuple[str, Optional[str]]]:
        """Find all usepackage declarations.
        
        Returns:
            List of (package_name, options) tuples
        """
        if self.use_ast and self._nodes:
            return self._find_packages_ast()
        return self._find_packages_regex()
    
    def _find_packages_ast(self) -> List[Tuple[str, Optional[str]]]:
        """Find packages using AST traversal."""
        packages = []
        for node in self._traverse_nodes(self._nodes):
            if isinstance(node, LatexMacroNode) and node.macroname == "usepackage":
                args = self._extract_macro_args(node)
                if args:
                    if len(args) >= 2:
                        packages.append((args[1], args[0]))  # (package, options)
                    elif len(args) == 1:
                        packages.append((args[0], None))
        return packages
    
    def _find_packages_regex(self) -> List[Tuple[str, Optional[str]]]:
        """Find packages using regex (fallback)."""
        # Skip comments
        lines = [line for line in self.text.split('\n') if not line.strip().startswith('%')]
        text_no_comments = '\n'.join(lines)
        
        packages = []
        pattern = r'\\usepackage\s*(?:\[([^\]]+)\])?\s*\{([^}]+)\}'
        for match in re.finditer(pattern, text_no_comments):
            package_name = match.group(2)
            options = match.group(1)
            packages.append((package_name, options))
        return packages
    
    def find_environment(self, env_name: str) -> Optional[Tuple[int, int]]:
        """Find an environment by name.
        
        Args:
            env_name: Name of environment (e.g., 'document', 'figure')
        
        Returns:
            Tuple of (start_pos, end_pos) or None if not found
        """
        if self.use_ast and self._nodes:
            return self._find_environment_ast(env_name)
        return self._find_environment_regex(env_name)
    
    def _find_environment_ast(self, env_name: str) -> Optional[Tuple[int, int]]:
        """Find environment using AST traversal."""
        for node in self._traverse_nodes(self._nodes):
            if isinstance(node, LatexEnvironmentNode) and node.environmentname == env_name:
                return node.pos, node.pos + node.len
        return None
    
    def _find_environment_regex(self, env_name: str) -> Optional[Tuple[int, int]]:
        """Find environment using regex (fallback)."""
        # Skip comments
        lines = []
        for line in self.text.split('\n'):
            if not line.strip().startswith('%'):
                lines.append(line)
        text_no_comments = '\n'.join(lines)
        
        pattern = rf'\\begin\{{{re.escape(env_name)}\}}.*?\\end\{{{re.escape(env_name)}\}}'
        match = re.search(pattern, text_no_comments, re.DOTALL)
        if match:
            # Map position back to original text
            return match.start(), match.end()
        return None
    
    def find_preamble_end(self) -> int:
        """Find position where preamble ends (begin{document}).
        
        Returns:
            Position of \\begin{document} or end of file
        """
        if self.use_ast and self._nodes:
            return self._find_preamble_end_ast()
        return self._find_preamble_end_regex()
    
    def _find_preamble_end_ast(self) -> int:
        """Find preamble end using AST."""
        for node in self._traverse_nodes(self._nodes):
            if isinstance(node, LatexEnvironmentNode) and node.environmentname == "document":
                return node.pos
        return len(self.text)
    
    def _find_preamble_end_regex(self) -> int:
        """Find preamble end using regex (fallback)."""
        match = re.search(r'\\begin\{document\}', self.text)
        return match.start() if match else len(self.text)
    
    def is_inside_comment(self, position: int) -> bool:
        """Check if a position is inside a comment.
        
        Args:
            position: Character position in text
        
        Returns:
            True if position is in a commented line
        """
        if self.use_ast and self._nodes:
            return self._is_inside_comment_ast(position)
        return self._is_inside_comment_regex(position)
    
    def _is_inside_comment_ast(self, position: int) -> bool:
        """Check if position is in comment using AST."""
        for node in self._traverse_nodes(self._nodes):
            if isinstance(node, LatexCommentNode):
                if node.pos <= position < node.pos + node.len:
                    return True
        return False
    
    def _is_inside_comment_regex(self, position: int) -> bool:
        """Check if position is in comment using line-based check."""
        # Find which line this position is on
        line_start = self.text.rfind('\n', 0, position) + 1
        line_end = self.text.find('\n', position)
        if line_end == -1:
            line_end = len(self.text)
        
        line = self.text[line_start:line_end]
        # Check if there's a % before this position in the line
        rel_pos = position - line_start
        comment_pos = line.find('%')
        return comment_pos != -1 and comment_pos < rel_pos
    
    def _traverse_nodes(self, nodes: List[LatexNode]) -> List[LatexNode]:
        """Recursively traverse all nodes in AST.
        
        Yields nodes in depth-first order.
        """
        result = []
        for node in nodes:
            result.append(node)
            # Recursively traverse child nodes
            if hasattr(node, 'nodelist') and node.nodelist:
                result.extend(self._traverse_nodes(node.nodelist))
        return result
    
    def _extract_macro_args(self, node: LatexMacroNode) -> List[str]:
        """Extract arguments from a macro node.
        
        Returns:
            List of argument strings (both optional and required)
        """
        args = []
        if not node.nodeargd:
            return args
        
        # Extract optional arguments (in brackets)
        if hasattr(node.nodeargd, 'argnlist'):
            for arg_node in node.nodeargd.argnlist:
                if arg_node:
                    arg_text = self.text[arg_node.pos:arg_node.pos + arg_node.len]
                    # Strip brackets/braces
                    arg_text = arg_text.strip('[]{}')
                    args.append(arg_text)
        
        return args
    
    @staticmethod
    def is_available() -> bool:
        """Check if AST parsing is available (pylatexenc installed)."""
        return PYLATEXENC_AVAILABLE
