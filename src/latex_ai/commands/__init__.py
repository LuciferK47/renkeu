"""Command registry and base classes for extensible LaTeX editing.

This module provides the infrastructure for registering and executing
LaTeX editing commands in a structured, extensible way.
"""
from __future__ import annotations
from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from typing import Any, Callable, Dict, List, Optional, Type
import re


@dataclass
class CommandMetadata:
    """Metadata about a registered command."""
    name: str
    description: str
    examples: List[str]
    handler_class: Type[Command]
    pattern: str  # Regex pattern to match command text


class Command(ABC):
    """Base class for all LaTeX editing commands.
    
    Subclasses must implement execute() and provide metadata via class attributes.
    """
    
    # Class attributes (override in subclasses)
    name: str = ""
    description: str = ""
    examples: List[str] = field(default_factory=list)
    pattern: str = ""  # Regex to extract arguments
    
    def __init__(self, document: Document) -> None:
        self.document = document
    
    @abstractmethod
    def execute(self, **kwargs) -> EditResult:
        """Execute the command and return result.
        
        Args:
            **kwargs: Parsed arguments from the command pattern
            
        Returns:
            EditResult with changed flag, message, and new_text
        """
        pass
    
    @classmethod
    def matches(cls, command_text: str) -> Optional[Dict[str, str]]:
        """Check if command text matches this command's pattern.
        
        Returns:
            Dictionary of extracted arguments, or None if no match
        """
        if not cls.pattern:
            return None
        match = re.match(cls.pattern, command_text.strip(), re.IGNORECASE)
        if match:
            return match.groupdict()
        return None
    
    def validate_args(self, **kwargs) -> Optional[str]:
        """Validate command arguments.
        
        Returns:
            Error message if validation fails, None if valid
        """
        return None  # Override in subclasses


class CommandRegistry:
    """Central registry for all LaTeX editing commands."""
    
    _commands: Dict[str, Type[Command]] = {}
    _patterns: List[tuple[re.Pattern, Type[Command]]] = []
    
    @classmethod
    def register(cls, command_class: Type[Command]) -> Type[Command]:
        """Register a command class.
        
        Usage:
            @CommandRegistry.register
            class SetMarginsCommand(Command):
                ...
        """
        if not command_class.name:
            raise ValueError(f"Command {command_class.__name__} must define 'name' attribute")
        
        cls._commands[command_class.name] = command_class
        if command_class.pattern:
            pattern = re.compile(command_class.pattern, re.IGNORECASE)
            cls._patterns.append((pattern, command_class))
        
        return command_class
    
    @classmethod
    def find_command(cls, command_text: str) -> Optional[tuple[Type[Command], Dict[str, str]]]:
        """Find matching command and extract arguments.
        
        Returns:
            Tuple of (command_class, args_dict) or None if no match
        """
        for pattern, command_class in cls._patterns:
            match = pattern.match(command_text.strip())
            if match:
                return (command_class, match.groupdict())
        return None
    
    @classmethod
    def get_command(cls, name: str) -> Optional[Type[Command]]:
        """Get command class by name."""
        return cls._commands.get(name)
    
    @classmethod
    def list_commands(cls) -> List[CommandMetadata]:
        """Get metadata for all registered commands."""
        return [
            CommandMetadata(
                name=cmd.name,
                description=cmd.description,
                examples=cmd.examples,
                handler_class=cmd,
                pattern=cmd.pattern
            )
            for cmd in cls._commands.values()
        ]
    
    @classmethod
    def clear(cls) -> None:
        """Clear all registered commands (useful for testing)."""
        cls._commands.clear()
        cls._patterns.clear()


# Import Document and EditResult from other modules
# These are forward declarations; actual imports happen after modules are defined
from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from ..document import Document
    from ..types import EditResult
