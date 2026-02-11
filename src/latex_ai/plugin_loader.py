"""Plugin loader system for LaTeX-AI.

Discovers and loads custom commands from ~/.latex-ai/plugins/.
"""
from __future__ import annotations

import importlib.util
import sys
from pathlib import Path
from typing import List, Optional, TYPE_CHECKING
import logging

if TYPE_CHECKING:
    from .commands import Command

logger = logging.getLogger(__name__)


class PluginLoader:
    """Discovers and loads command plugins."""
    
    DEFAULT_PLUGIN_DIR = Path.home() / ".latex-ai" / "plugins"
    
    def __init__(self, plugin_dir: Optional[Path] = None):
        """Initialize plugin loader.
        
        Args:
            plugin_dir: Directory to search for plugins (defaults to ~/.latex-ai/plugins)
        """
        self.plugin_dir = plugin_dir or self.DEFAULT_PLUGIN_DIR
        self._loaded_plugins: List[str] = []
    
    def discover_plugins(self) -> List[Path]:
        """Discover available plugin directories.
        
        Returns:
            List of plugin directory paths
        """
        if not self.plugin_dir.exists():
            logger.info(f"Plugin directory does not exist: {self.plugin_dir}")
            return []
        
        plugins = []
        for item in self.plugin_dir.iterdir():
            if item.is_dir() and (item / "__init__.py").exists():
                plugins.append(item)
                logger.debug(f"Discovered plugin: {item.name}")
        
        return plugins
    
    def load_plugin(self, plugin_path: Path) -> bool:
        """Load a single plugin.
        
        Args:
            plugin_path: Path to plugin directory
        
        Returns:
            True if loaded successfully, False otherwise
        """
        plugin_name = plugin_path.name
        
        # Skip if already loaded
        if plugin_name in self._loaded_plugins:
            logger.debug(f"Plugin already loaded: {plugin_name}")
            return True
        
        try:
            # Import the plugin module
            spec = importlib.util.spec_from_file_location(
                f"latex_ai_plugin_{plugin_name}",
                plugin_path / "__init__.py"
            )
            
            if spec is None or spec.loader is None:
                logger.error(f"Failed to load plugin spec: {plugin_name}")
                return False
            
            module = importlib.util.module_from_spec(spec)
            sys.modules[spec.name] = module
            spec.loader.exec_module(module)
            
            self._loaded_plugins.append(plugin_name)
            logger.info(f"Successfully loaded plugin: {plugin_name}")
            return True
            
        except Exception as e:
            logger.error(f"Error loading plugin {plugin_name}: {e}", exc_info=True)
            return False
    
    def load_all_plugins(self) -> int:
        """Load all discovered plugins.
        
        Returns:
            Number of plugins successfully loaded
        """
        plugins = self.discover_plugins()
        loaded_count = 0
        
        for plugin_path in plugins:
            if self.load_plugin(plugin_path):
                loaded_count += 1
        
        logger.info(f"Loaded {loaded_count}/{len(plugins)} plugins")
        return loaded_count
    
    def validate_plugin(self, plugin_path: Path) -> bool:
        """Validate that a plugin directory is properly structured.
        
        Args:
            plugin_path: Path to plugin directory
        
        Returns:
            True if valid, False otherwise
        """
        # Check for __init__.py
        if not (plugin_path / "__init__.py").exists():
            logger.warning(f"Plugin missing __init__.py: {plugin_path.name}")
            return False
        
        # Check that it's a directory
        if not plugin_path.is_dir():
            logger.warning(f"Plugin path is not a directory: {plugin_path}")
            return False
        
        return True
    
    def create_plugin_directory(self, plugin_name: str) -> Path:
        """Create a new plugin directory structure.
        
        Args:
            plugin_name: Name of the plugin
        
        Returns:
            Path to created plugin directory
        """
        plugin_dir = self.plugin_dir / plugin_name
        plugin_dir.mkdir(parents=True, exist_ok=True)
        
        # Create __init__.py with template
        init_file = plugin_dir / "__init__.py"
        if not init_file.exists():
            template = '''"""Custom LaTeX-AI plugin: {name}

Add your custom commands here using @CommandRegistry.register decorator.
"""
from latex_ai.commands import Command, CommandRegistry
from latex_ai.types import EditResult


@CommandRegistry.register
class ExampleCommand(Command):
    """Example custom command."""
    name = "example"
    description = "Example plugin command"
    pattern = r"^example\\s+(?P<arg>\\w+)$"
    examples = ["latex-ai apply file.tex \\"example test\\""]
    
    def execute(self, **kwargs) -> EditResult:
        arg = kwargs["arg"]
        # Your command logic here
        return EditResult(
            changed=False,
            message=f"Example: {{arg}}"
        )
'''.format(name=plugin_name)
            
            init_file.write_text(template, encoding="utf-8")
        
        logger.info(f"Created plugin directory: {plugin_dir}")
        return plugin_dir
    
    @property
    def loaded_plugins(self) -> List[str]:
        """Get list of loaded plugin names."""
        return self._loaded_plugins.copy()


# Global plugin loader instance
_plugin_loader: Optional[PluginLoader] = None


def get_plugin_loader() -> PluginLoader:
    """Get the global plugin loader instance."""
    global _plugin_loader
    if _plugin_loader is None:
        _plugin_loader = PluginLoader()
    return _plugin_loader


def load_plugins() -> int:
    """Load all plugins from default directory.
    
    Returns:
        Number of plugins loaded
    """
    loader = get_plugin_loader()
    return loader.load_all_plugins()
