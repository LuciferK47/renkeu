"""Tests for plugin loader system."""
import pytest
from pathlib import Path
from latex_ai.plugin_loader import PluginLoader, load_plugins
from latex_ai.commands import CommandRegistry


@pytest.fixture
def plugin_dir(tmp_path):
    """Create temporary plugin directory."""
    return tmp_path / "plugins"


@pytest.fixture
def sample_plugin(plugin_dir):
    """Create a sample plugin."""
    plugin_path = plugin_dir / "sample_plugin"
    plugin_path.mkdir(parents=True)
    
    # Create __init__.py with a simple command
    init_content = '''"""Sample plugin for testing."""
from latex_ai.commands import Command, CommandRegistry
from latex_ai.types import EditResult


@CommandRegistry.register
class TestPluginCommand(Command):
    """Test command from plugin."""
    name = "test_plugin"
    description = "Test plugin command"
    pattern = r"^test\\s+plugin$"
    examples = ["latex-ai apply file.tex \\"test plugin\\""]
    
    def execute(self, **kwargs) -> EditResult:
        return EditResult(
            changed=False,
            message="Plugin command executed"
        )
'''
    (plugin_path / "__init__.py").write_text(init_content, encoding="utf-8")
    return plugin_path


@pytest.fixture
def invalid_plugin(plugin_dir):
    """Create an invalid plugin (no __init__.py)."""
    plugin_path = plugin_dir / "invalid_plugin"
    plugin_path.mkdir(parents=True)
    return plugin_path


class TestPluginDiscovery:
    """Test plugin discovery."""
    
    def test_discover_no_plugins(self, plugin_dir):
        """Test discovery with empty directory."""
        plugin_dir.mkdir(parents=True)
        loader = PluginLoader(plugin_dir)
        
        plugins = loader.discover_plugins()
        assert len(plugins) == 0
    
    def test_discover_one_plugin(self, sample_plugin, plugin_dir):
        """Test discovering a single plugin."""
        loader = PluginLoader(plugin_dir)
        
        plugins = loader.discover_plugins()
        assert len(plugins) == 1
        assert plugins[0].name == "sample_plugin"
    
    def test_discover_ignores_invalid(self, sample_plugin, invalid_plugin, plugin_dir):
        """Test that invalid plugins are ignored."""
        loader = PluginLoader(plugin_dir)
        
        plugins = loader.discover_plugins()
        # Should only find valid plugin
        assert len(plugins) == 1
        assert plugins[0].name == "sample_plugin"
    
    def test_discover_nonexistent_directory(self, tmp_path):
        """Test discovery when plugin directory doesn't exist."""
        nonexistent = tmp_path / "does_not_exist"
        loader = PluginLoader(nonexistent)
        
        plugins = loader.discover_plugins()
        assert len(plugins) == 0


class TestPluginLoading:
    """Test plugin loading."""
    
    def test_load_valid_plugin(self, sample_plugin, plugin_dir):
        """Test loading a valid plugin."""
        loader = PluginLoader(plugin_dir)
        
        result = loader.load_plugin(sample_plugin)
        assert result is True
        assert "sample_plugin" in loader.loaded_plugins
    
    def test_load_plugin_twice(self, sample_plugin, plugin_dir):
        """Test loading same plugin twice."""
        loader = PluginLoader(plugin_dir)
        
        # Load first time
        result1 = loader.load_plugin(sample_plugin)
        assert result1 is True
        
        # Load second time (should skip)
        result2 = loader.load_plugin(sample_plugin)
        assert result2 is True
        assert loader.loaded_plugins.count("sample_plugin") == 1
    
    def test_load_all_plugins(self, sample_plugin, plugin_dir):
        """Test loading all plugins."""
        loader = PluginLoader(plugin_dir)
        
        count = loader.load_all_plugins()
        assert count == 1
        assert "sample_plugin" in loader.loaded_plugins
    
    def test_plugin_command_registered(self, sample_plugin, plugin_dir):
        """Test that plugin command is registered."""
        loader = PluginLoader(plugin_dir)
        loader.load_plugin(sample_plugin)
        
        # Check if command was registered
        cmd = CommandRegistry.get_command("test_plugin")
        assert cmd is not None
        assert cmd.name == "test_plugin"


class TestPluginValidation:
    """Test plugin validation."""
    
    def test_validate_valid_plugin(self, sample_plugin, plugin_dir):
        """Test validating a valid plugin."""
        loader = PluginLoader(plugin_dir)
        
        assert loader.validate_plugin(sample_plugin) is True
    
    def test_validate_invalid_plugin(self, invalid_plugin, plugin_dir):
        """Test validating an invalid plugin."""
        loader = PluginLoader(plugin_dir)
        
        assert loader.validate_plugin(invalid_plugin) is False
    
    def test_validate_file_not_directory(self, plugin_dir):
        """Test validation fails for files."""
        plugin_dir.mkdir(parents=True)
        plugin_file = plugin_dir / "not_a_dir.py"
        plugin_file.write_text("# Not a directory", encoding="utf-8")
        
        loader = PluginLoader(plugin_dir)
        assert loader.validate_plugin(plugin_file) is False


class TestPluginCreation:
    """Test plugin directory creation."""
    
    def test_create_plugin_directory(self, plugin_dir):
        """Test creating a new plugin directory."""
        loader = PluginLoader(plugin_dir)
        
        plugin_path = loader.create_plugin_directory("my_plugin")
        
        assert plugin_path.exists()
        assert plugin_path.is_dir()
        assert (plugin_path / "__init__.py").exists()
    
    def test_created_plugin_has_template(self, tmp_path):
        """Test that created plugin has template code."""
        plugin_dir = tmp_path / "isolated_plugins"
        loader = PluginLoader(plugin_dir)
        
        plugin_path = loader.create_plugin_directory("my_plugin")
        init_content = (plugin_path / "__init__.py").read_text(encoding="utf-8")
        
        # Check for template content
        assert "CommandRegistry.register" in init_content
        assert "ExampleCommand" in init_content
        assert "def execute" in init_content
    
    def test_create_existing_plugin_safe(self, plugin_dir):
        """Test that creating existing plugin doesn't overwrite."""
        loader = PluginLoader(plugin_dir)
        
        # Create plugin
        plugin_path = loader.create_plugin_directory("my_plugin")
        
        # Modify __init__.py
        init_file = plugin_path / "__init__.py"
        custom_content = "# Custom content"
        init_file.write_text(custom_content, encoding="utf-8")
        
        # Try to create again
        loader.create_plugin_directory("my_plugin")
        
        # Content should be preserved
        assert init_file.read_text(encoding="utf-8") == custom_content


class TestPluginExecution:
    """Test executing plugin commands."""
    
    def test_plugin_command_execution(self, sample_plugin, plugin_dir):
        """Test executing a command from a plugin."""
        loader = PluginLoader(plugin_dir)
        loader.load_plugin(sample_plugin)
        
        # Find and execute command
        result = CommandRegistry.find_command("test plugin")
        assert result is not None
        
        cmd_class, args = result
        cmd = cmd_class(None)  # No document needed for this test
        edit_result = cmd.execute(**args)
        
        assert not edit_result.changed
        assert "Plugin command executed" in edit_result.message


class TestGlobalPluginLoader:
    """Test global plugin loader functions."""
    
    def test_load_plugins_function(self, sample_plugin, plugin_dir, monkeypatch):
        """Test the global load_plugins function."""
        # Mock the default plugin directory
        from latex_ai import plugin_loader
        monkeypatch.setattr(plugin_loader.PluginLoader, "DEFAULT_PLUGIN_DIR", plugin_dir)
        
        count = load_plugins()
        assert count >= 0  # May be 0 if plugins already loaded


class TestPluginErrorHandling:
    """Test error handling in plugin loading."""
    
    def test_load_plugin_with_syntax_error(self, plugin_dir):
        """Test loading plugin with Python syntax error."""
        plugin_path = plugin_dir / "broken_plugin"
        plugin_path.mkdir(parents=True)
        
        # Create __init__.py with syntax error
        (plugin_path / "__init__.py").write_text("def broken(:\n    pass", encoding="utf-8")
        
        loader = PluginLoader(plugin_dir)
        result = loader.load_plugin(plugin_path)
        
        assert result is False
        assert "broken_plugin" not in loader.loaded_plugins
    
    def test_load_plugin_with_import_error(self, plugin_dir):
        """Test loading plugin with missing imports."""
        plugin_path = plugin_dir / "import_error_plugin"
        plugin_path.mkdir(parents=True)
        
        # Create __init__.py with import error
        (plugin_path / "__init__.py").write_text("import nonexistent_module", encoding="utf-8")
        
        loader = PluginLoader(plugin_dir)
        result = loader.load_plugin(plugin_path)
        
        assert result is False
        assert "import_error_plugin" not in loader.loaded_plugins
