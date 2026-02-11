"""Configuration system for LaTeX-AI."""

import toml
from pathlib import Path
from typing import Dict, Any, Optional, List
from dataclasses import dataclass, field
from pydantic import BaseModel, Field, field_validator
from pydantic_settings import BaseSettings, SettingsConfigDict


@dataclass
class KeyboardShortcuts:
    """Keyboard shortcuts configuration."""
    
    save: str = "ctrl+s"
    undo: str = "ctrl+z"
    compile: str = "ctrl+enter"
    apply_command: str = "alt+a"
    toggle_history: str = "alt+h"
    toggle_file_tree: str = "alt+f"
    
    def validate(self) -> Dict[str, str]:
        """
        Validate keyboard shortcuts for conflicts and invalid formats.
        
        Returns:
            Dictionary of errors (empty if valid)
        """
        errors = {}
        shortcuts = {
            "save": self.save,
            "undo": self.undo,
            "compile": self.compile,
            "apply_command": self.apply_command,
            "toggle_history": self.toggle_history,
            "toggle_file_tree": self.toggle_file_tree,
        }
        
        # Check for duplicates
        seen = {}
        for name, shortcut in shortcuts.items():
            if shortcut in seen:
                errors[name] = f"Duplicate shortcut '{shortcut}' already used by '{seen[shortcut]}'"
            else:
                seen[shortcut] = name
        
        # Validate format (modifier+key)
        import re
        valid_pattern = re.compile(r"^(ctrl|alt|shift)(\+(ctrl|alt|shift))*\+[a-z0-9]+(enter|space|tab|escape)?$")
        for name, shortcut in shortcuts.items():
            if not valid_pattern.match(shortcut.lower()):
                errors[name] = f"Invalid shortcut format '{shortcut}'. Use format: modifier+key (e.g., ctrl+s, alt+enter)"
        
        return errors


@dataclass
class ThemeConfig:
    """Theme configuration."""
    
    mode: str = "light"  # light, dark, auto
    primary_color: str = "#007bff"
    accent_color: str = "#28a745"
    error_color: str = "#c62828"


@dataclass
class EditorConfig:
    """Editor configuration."""
    
    font_size: int = 14
    font_family: str = "monospace"
    theme: str = "vs-light"
    tab_size: int = 2
    word_wrap: bool = True
    minimap_enabled: bool = False
    ignore_patterns: list = field(default_factory=lambda: [
        '.git', 'node_modules', '.venv', 'dist', 'build',
        '__pycache__', '.pytest_cache', 'venv', '.mypy_cache'
    ])


@dataclass
class Config:
    """Main configuration."""
    
    keyboard_shortcuts: KeyboardShortcuts = field(default_factory=KeyboardShortcuts)
    theme: ThemeConfig = field(default_factory=ThemeConfig)
    editor: EditorConfig = field(default_factory=EditorConfig)
    auto_save: bool = True
    auto_save_interval: int = 10  # seconds (optimized for bandwidth savings)
    history_limit: int = 100
    max_file_size: int = 10 * 1024 * 1024  # 10 MB
    workspace_root: Optional[str] = None  # Root directory for file access validation
    ai_model_default: str = "gemini-pro"  # Default must exist in registry
    
    # Resource limits
    max_concurrent_compilations: int = 3
    compilation_timeout: int = 120  # seconds
    max_ai_context_size: int = 100000  # characters
    max_websocket_connections: int = 50
    

class AppSettings(BaseSettings):
    """Application settings with environment variable support.
    
    Loads from environment variables prefixed with LATEX_AI_
    Example: LATEX_AI_MAX_FILE_SIZE=20971520
    """
    model_config = SettingsConfigDict(
        env_prefix='LATEX_AI_',
        env_file='.env',
        env_file_encoding='utf-8',
        case_sensitive=False
    )
    
    # Core settings
    max_file_size: int = Field(default=10 * 1024 * 1024, description="Max file size in bytes")
    max_concurrent_compilations: int = Field(default=3, description="Max concurrent compilations")
    compilation_timeout: int = Field(default=120, description="Compilation timeout in seconds")
    max_ai_context_size: int = Field(default=100000, description="Max AI context size in chars")
    max_websocket_connections: int = Field(default=50, description="Max WebSocket connections")
    
    # Cache settings
    max_compile_cache_entries: int = Field(default=20, description="Max compile cache entries")
    
    # API keys (optional, can be set via env)
    gemini_api_key: Optional[str] = Field(default=None, description="Google Gemini API key")
    
    # Server settings
    host: str = Field(default="127.0.0.1", description="Server host")
    port: int = Field(default=8000, description="Server port")
    workers: int = Field(default=1, description="Number of uvicorn workers")
    
    # Paths
    workspace_root: Optional[str] = Field(default=None, description="Workspace root directory")
    
    @field_validator('workspace_root')
    @classmethod
    def set_workspace_root(cls, v):
        """Set default workspace root to current directory if not specified."""
        return v or str(Path.cwd())


# Global settings instance
_app_settings: Optional[AppSettings] = None


def get_app_settings() -> AppSettings:
    """Get or create global app settings."""
    global _app_settings
    if _app_settings is None:
        _app_settings = AppSettings()
    return _app_settings
    

    # AI retry configuration
    ai_max_retries: int = 3
    ai_retry_delay: float = 1.0  # seconds (exponential backoff)
    ai_timeout: int = 60  # seconds per request


class ConfigManager:
    """Manages application configuration."""
    
    def __init__(self, config_file: Optional[str] = None):
        """
        Initialize config manager.
        
        Args:
            config_file: Path to .latex-ai.toml config file
                        (default: ~/.latex-ai/.latex-ai.toml)
        """
        if config_file:
            self.config_file = Path(config_file)
        else:
            self.config_file = Path.home() / ".latex-ai" / ".latex-ai.toml"
        
        self.config_file.parent.mkdir(parents=True, exist_ok=True)
        self._config = self._load_or_create_config()
    
    def _load_or_create_config(self) -> Config:
        """Load configuration from file or create default."""
        if self.config_file.exists():
            return self._load_config()
        else:
            config = Config()
            # Ensure workspace_root defaults to current working directory
            if not config.workspace_root:
                config.workspace_root = str(Path.cwd())
            self._save_config(config)
            return config
    
    def _load_config(self) -> Config:
        """Load configuration from TOML file."""
        try:
            data = toml.load(self.config_file)
            config = Config()
            
            # Update from file
            if 'keyboard_shortcuts' in data:
                shortcuts_data = data['keyboard_shortcuts']
                config.keyboard_shortcuts = KeyboardShortcuts(
                    save=shortcuts_data.get('save', 'ctrl+s'),
                    undo=shortcuts_data.get('undo', 'ctrl+z'),
                    compile=shortcuts_data.get('compile', 'ctrl+enter'),
                    apply_command=shortcuts_data.get('apply_command', 'alt+a'),
                    toggle_history=shortcuts_data.get('toggle_history', 'alt+h'),
                    toggle_file_tree=shortcuts_data.get('toggle_file_tree', 'alt+f')
                )
            
            if 'theme' in data:
                theme_data = data['theme']
                config.theme = ThemeConfig(
                    mode=theme_data.get('mode', 'light'),
                    primary_color=theme_data.get('primary_color', '#007bff'),
                    accent_color=theme_data.get('accent_color', '#28a745'),
                    error_color=theme_data.get('error_color', '#c62828')
                )
            
            if 'editor' in data:
                editor_data = data['editor']
                config.editor = EditorConfig(
                    font_size=editor_data.get('font_size', 14),
                    font_family=editor_data.get('font_family', 'monospace'),
                    theme=editor_data.get('theme', 'vs-light'),
                    tab_size=editor_data.get('tab_size', 2),
                    word_wrap=editor_data.get('word_wrap', True),
                    minimap_enabled=editor_data.get('minimap_enabled', False),
                    ignore_patterns=editor_data.get('ignore_patterns', [
                        '.git', 'node_modules', '.venv', 'dist', 'build',
                        '__pycache__', '.pytest_cache', 'venv', '.mypy_cache'
                    ])
                )
            
            config.auto_save = data.get('auto_save', True)
            config.auto_save_interval = data.get('auto_save_interval', 30)
            config.history_limit = data.get('history_limit', 100)
            config.max_file_size = data.get('max_file_size', 10 * 1024 * 1024)
            config.workspace_root = data.get('workspace_root')
            config.ai_model_default = data.get('ai_model_default', 'gpt-5.1-codex-max')
            
            # If workspace_root is missing or empty, default to cwd and persist
            if not config.workspace_root:
                config.workspace_root = str(Path.cwd())
                self._save_config(config)
            
            return config
        except Exception as e:
            print(f"Error loading config: {e}. Using defaults.")
            return Config()
    
    def _save_config(self, config: Config) -> None:
        """Save configuration to TOML file."""
        data = {
            'keyboard_shortcuts': {
                'save': config.keyboard_shortcuts.save,
                'undo': config.keyboard_shortcuts.undo,
                'compile': config.keyboard_shortcuts.compile,
                'apply_command': config.keyboard_shortcuts.apply_command,
                'toggle_history': config.keyboard_shortcuts.toggle_history,
                'toggle_file_tree': config.keyboard_shortcuts.toggle_file_tree,
            },
            'theme': {
                'mode': config.theme.mode,
                'primary_color': config.theme.primary_color,
                'accent_color': config.theme.accent_color,
                'error_color': config.theme.error_color,
            },
            'editor': {
                'font_size': config.editor.font_size,
                'font_family': config.editor.font_family,
                'theme': config.editor.theme,
                'tab_size': config.editor.tab_size,
                'word_wrap': config.editor.word_wrap,
                'minimap_enabled': config.editor.minimap_enabled,
                'ignore_patterns': config.editor.ignore_patterns,
            },
            'auto_save': config.auto_save,
            'auto_save_interval': config.auto_save_interval,
            'history_limit': config.history_limit,
            'max_file_size': config.max_file_size,
            'workspace_root': config.workspace_root,
            'ai_model_default': config.ai_model_default,
        }
        
        with open(self.config_file, 'w') as f:
            toml.dump(data, f)
    
    def get_config(self) -> Config:
        """Get current configuration."""
        return self._config
    
    def update_config(self, **kwargs) -> None:
        """Update configuration values."""
        config = self._config
        
        for key, value in kwargs.items():
            if hasattr(config, key):
                setattr(config, key, value)
        
        self._save_config(config)
    
    def get_keyboard_shortcut(self, action: str) -> str:
        """Get keyboard shortcut for action."""
        shortcuts = self._config.keyboard_shortcuts
        return getattr(shortcuts, action, None)


# Global config manager instance
_config_manager: Optional[ConfigManager] = None


def get_config_manager() -> ConfigManager:
    """Get or create global config manager."""
    global _config_manager
    if _config_manager is None:
        _config_manager = ConfigManager()
    return _config_manager


def get_config() -> Config:
    """Get current configuration."""
    return get_config_manager().get_config()
