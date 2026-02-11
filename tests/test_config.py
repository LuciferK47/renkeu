"""Tests for configuration system and API endpoints."""

import json
import tempfile
from pathlib import Path
import pytest
from fastapi.testclient import TestClient

from latex_ai.config import Config, ConfigManager, KeyboardShortcuts, ThemeConfig, EditorConfig
from latex_ai.server import app, ConfigResponse


# ============================================================================
# Fixtures
# ============================================================================

@pytest.fixture
def config_file():
    """Create temporary config file for testing."""
    with tempfile.NamedTemporaryFile(mode='w', suffix='.toml', delete=False) as f:
        temp_path = f.name
    yield temp_path
    # Cleanup
    Path(temp_path).unlink(missing_ok=True)


@pytest.fixture
def client():
    """Create test client."""
    return TestClient(app)


# ============================================================================
# Configuration Tests
# ============================================================================

class TestConfig:
    """Test configuration dataclasses."""

    def test_keyboard_shortcuts_defaults(self):
        """Test KeyboardShortcuts has sensible defaults."""
        shortcuts = KeyboardShortcuts()
        assert shortcuts.save == "ctrl+s"
        assert shortcuts.undo == "ctrl+z"
        assert shortcuts.compile == "ctrl+enter"
        assert shortcuts.apply_command == "alt+a"
        assert shortcuts.toggle_history == "alt+h"
        assert shortcuts.toggle_file_tree == "alt+f"

    def test_theme_config_defaults(self):
        """Test ThemeConfig has sensible defaults."""
        theme = ThemeConfig()
        assert theme.mode == "light"
        assert theme.primary_color == "#007bff"
        assert theme.accent_color == "#28a745"
        assert theme.error_color == "#c62828"

    def test_editor_config_defaults(self):
        """Test EditorConfig has sensible defaults."""
        editor = EditorConfig()
        assert editor.font_size == 14
        assert editor.font_family == "monospace"
        assert editor.tab_size == 2
        assert editor.word_wrap is True
        assert editor.minimap_enabled is False

    def test_config_defaults(self):
        """Test main Config has all components."""
        config = Config()
        assert isinstance(config.keyboard_shortcuts, KeyboardShortcuts)
        assert isinstance(config.theme, ThemeConfig)
        assert isinstance(config.editor, EditorConfig)
        assert config.auto_save is True
        assert config.auto_save_interval == 30
        assert config.history_limit == 100


class TestConfigManager:
    """Test configuration manager."""

    def test_config_manager_creates_default(self, config_file):
        """Test ConfigManager creates default config if missing."""
        manager = ConfigManager(config_file)
        assert manager.get_config() is not None
        assert Path(config_file).exists()

    def test_config_manager_saves_and_loads(self, config_file):
        """Test ConfigManager saves and loads configuration."""
        # Create and save config
        manager1 = ConfigManager(config_file)
        config1 = manager1.get_config()
        config1.editor.font_size = 18
        manager1._save_config(config1)

        # Load config in new manager
        manager2 = ConfigManager(config_file)
        config2 = manager2.get_config()
        assert config2.editor.font_size == 18

    def test_config_manager_update_config(self, config_file):
        """Test ConfigManager update_config method."""
        manager = ConfigManager(config_file)
        config = manager.get_config()
        
        config.editor.tab_size = 4
        config.theme.mode = "dark"
        manager._save_config(config)

        # Reload and verify
        manager2 = ConfigManager(config_file)
        config2 = manager2.get_config()
        assert config2.editor.tab_size == 4
        assert config2.theme.mode == "dark"


# ============================================================================
# API Endpoint Tests
# ============================================================================

class TestConfigEndpoints:
    """Test /api/config endpoints."""

    def test_get_config_endpoint(self, client):
        """Test GET /api/config returns ConfigResponse."""
        response = client.get("/api/config")
        assert response.status_code == 200
        
        data = response.json()
        assert "keyboard_shortcuts" in data
        assert "theme" in data
        assert "editor" in data
        assert "auto_save" in data
        assert "history_limit" in data

        # Verify keyboard shortcuts structure
        assert "save" in data["keyboard_shortcuts"]
        assert "undo" in data["keyboard_shortcuts"]
        assert "compile" in data["keyboard_shortcuts"]

        # Verify theme structure
        assert "mode" in data["theme"]
        assert "primary_color" in data["theme"]
        assert "accent_color" in data["theme"]

        # Verify editor structure
        assert "font_size" in data["editor"]
        assert "tab_size" in data["editor"]
        assert "word_wrap" in data["editor"]

    def test_post_config_endpoint_flat_update(self, client):
        """Test POST /api/config with flat update."""
        update_data = {
            "auto_save": False,
            "history_limit": 50
        }
        
        response = client.post("/api/config", json=update_data)
        assert response.status_code == 200
        
        data = response.json()
        assert data["success"] is True
        assert "config" in data

    def test_post_config_endpoint_nested_update(self, client):
        """Test POST /api/config with nested updates."""
        update_data = {
            "theme.mode": "dark",
            "editor.tab_size": 4,
            "keyboard_shortcuts.save": "alt+s"
        }
        
        response = client.post("/api/config", json=update_data)
        assert response.status_code == 200
        
        data = response.json()
        assert data["success"] is True
        assert data["config"]["theme"]["mode"] == "dark"
        assert data["config"]["editor"]["tab_size"] == 4

    def test_post_config_endpoint_mixed_update(self, client):
        """Test POST /api/config with mixed flat and nested updates."""
        update_data = {
            "auto_save": True,
            "theme.mode": "light",
            "editor.font_size": 16
        }
        
        response = client.post("/api/config", json=update_data)
        assert response.status_code == 200
        
        data = response.json()
        assert data["success"] is True

    def test_post_config_endpoint_invalid_update(self, client):
        """Test POST /api/config with invalid keys."""
        # This should still work but not update anything
        update_data = {
            "invalid_key": "value"
        }
        
        response = client.post("/api/config", json=update_data)
        # Should either ignore or handle gracefully
        assert response.status_code in [200, 400]


# ============================================================================
# Integration Tests
# ============================================================================

class TestConfigIntegration:
    """Test configuration system integration."""

    def test_config_changes_persist(self, client):
        """Test that config changes made via API persist."""
        # Get initial config
        response1 = client.get("/api/config")
        initial_font_size = response1.json()["editor"]["font_size"]

        # Update font size
        new_font_size = initial_font_size + 2
        response2 = client.post("/api/config", json={
            "editor.font_size": new_font_size
        })
        assert response2.status_code == 200

        # Verify persisted
        response3 = client.get("/api/config")
        assert response3.json()["editor"]["font_size"] == new_font_size

    def test_multiple_updates(self, client):
        """Test multiple sequential updates."""
        updates_list = [
            {"theme.mode": "dark"},
            {"editor.tab_size": 4},
            {"auto_save": False}
        ]

        for updates in updates_list:
            response = client.post("/api/config", json=updates)
            assert response.status_code == 200

        # Verify all updates persisted
        response = client.get("/api/config")
        data = response.json()
        assert data["theme"]["mode"] == "dark"
        assert data["editor"]["tab_size"] == 4
        assert data["auto_save"] is False


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
