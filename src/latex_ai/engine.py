from __future__ import annotations
import re
import difflib
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Callable, Dict, List, Optional, Tuple

from .types import EditResult
from .document import Document
from .commands import CommandRegistry
# Import builtin commands to register them
from .commands import builtin  # noqa: F401

# Minimal helpers; deeper parsing will use pylatexenc later
_GEO_PKG_PATTERN = re.compile(r"\\usepackage\[(?P<opts>[^\]]*)\]\{geometry\}")
_GEO_PKG_NOOPTS_PATTERN = re.compile(r"\\usepackage\{geometry\}")
_DOCCLASS_PATTERN = re.compile(r"^\\documentclass\s*(\[(?P<opts>[^\]]*)\])?\s*\{(?P<class>[^}]+)\}", re.M)
_INCLUDEGRAPHICS_PATTERN = re.compile(r"\\includegraphics\s*(\[(?P<opts>[^\]]*)\])?\s*\{(?P<file>[^}]+)\}")

class LatexEngine:
    def __init__(self, path: Path, use_registry: bool = True) -> None:
        self.path = Path(path)
        self.text = self.path.read_text(encoding="utf-8")
        self._max_backups = 10
        self.use_registry = use_registry
        self.document = Document(self.path, self.text)

    def reload(self) -> None:
        """Reload file from disk. Critical for REPL mode."""
        try:
            self.text = self.path.read_text(encoding="utf-8")
            self.document = Document(self.path, self.text)
        except Exception as e:
            raise IOError(f"Failed to reload {self.path}: {e}")

    # High-level API
    def compute_diff(self, new_text: str) -> str:
        """Generate unified diff between current and new text."""
        old_lines = self.text.splitlines(keepends=True)
        new_lines = new_text.splitlines(keepends=True)
        diff = difflib.unified_diff(
            old_lines,
            new_lines,
            fromfile=f"{self.path} (original)",
            tofile=f"{self.path} (modified)",
            lineterm=""
        )
        return "".join(diff)

    def apply_command(self, command: str, auto_save: bool = False) -> EditResult:
        """Parse and execute command. Returns EditResult with new_text.
        
        If auto_save=True, writes to disk (legacy behavior).
        If auto_save=False, caller must handle save.
        """
        # Try registry system first (if enabled)
        if self.use_registry:
            result = CommandRegistry.find_command(command)
            if result:
                command_class, args = result
                cmd_instance = command_class(self.document)
                edit_result = cmd_instance.execute(**args)
                
                # Auto-save if requested (legacy mode)
                if auto_save and edit_result.changed and edit_result.new_text:
                    self.text = edit_result.new_text
                    self.document.text = edit_result.new_text
                    self.save(backup=True)
                
                return edit_result
        
        # Fallback to legacy command parsing
        cmd = command.strip().lower()
        
        # Validate and route to handlers
        if cmd.startswith("set margins "):
            size = command.strip()[len("set margins "):].strip()
            if not self._validate_size(size):
                return EditResult(False, f"Invalid size format: {size}")
            result = self._set_margins(size)
        elif cmd.startswith("set line spacing "):
            value = command.strip()[len("set line spacing "):].strip()
            if not self._validate_spacing(value):
                return EditResult(False, f"Invalid spacing value: {value} (must be 1.0-3.0)")
            result = self._set_line_spacing(value)
        elif cmd.startswith("set figure width "):
            value = command.strip()[len("set figure width "):].strip()
            result = self._set_figure_width(value)
        elif cmd.startswith("change documentclass to "):
            value = command.strip()[len("change documentclass to "):].strip()
            if not self._validate_documentclass(value):
                return EditResult(False, f"Invalid documentclass: {value}")
            result = self._set_documentclass(value)
        elif cmd == "normalize whitespace":
            result = self._normalize_whitespace()
        elif cmd.startswith("add package "):
            pkg = command.strip()[len("add package "):].strip()
            result = self._add_package(pkg)
        elif cmd.startswith("remove package "):
            pkg = command.strip()[len("remove package "):].strip()
            result = self._remove_package(pkg)
        else:
            return EditResult(False, f"Unknown command: {command}")
        
        # Auto-save if requested (legacy mode)
        if auto_save and result.changed and result.new_text:
            self.text = result.new_text
            self.save(backup=True)
        
        return result

    def preview(self) -> Dict[str, str]:
        docclass = self._extract_documentclass()
        geometry = self._extract_geometry()
        return {
            "documentclass": docclass or "(not found)",
            "geometry": geometry or "(not found)",
        }

    def save(self, backup: bool = True) -> Optional[Path]:
        """Save current text to file with optional timestamped backup."""
        backup_path = None
        try:
            if backup:
                # Create timestamped backup
                timestamp = datetime.now().strftime("%Y%m%dT%H%M%S")
                backup_path = self.path.with_suffix(f"{self.path.suffix}.bak.{timestamp}")
                backup_path.write_text(self.text, encoding="utf-8")
                
                # Cleanup old backups (keep last N)
                self._cleanup_old_backups()
            
            self.path.write_text(self.text, encoding="utf-8")
            return backup_path
        except Exception as e:
            raise IOError(f"Failed to save {self.path}: {e}")

    def _cleanup_old_backups(self) -> None:
        """Keep only the most recent N backups."""
        pattern = f"{self.path.name}.bak.*"
        backups = sorted(self.path.parent.glob(pattern), key=lambda p: p.stat().st_mtime)
        
        # Remove oldest backups if we exceed limit
        while len(backups) > self._max_backups:
            oldest = backups.pop(0)
            try:
                oldest.unlink()
            except Exception:
                pass  # Ignore cleanup failures

    # Command handlers (return new_text, do not auto-save)
    def _set_margins(self, size: str) -> EditResult:
        # Ensure geometry package with margin option
        replaced = False
        def repl(match: re.Match) -> str:
            nonlocal replaced
            replaced = True
            opts = match.group("opts") or ""
            opts_map = self._options_to_map(opts)
            opts_map["margin"] = size
            new_opts = self._map_to_options(opts_map)
            return f"\\usepackage[{new_opts}]{{geometry}}"

        new_text = _GEO_PKG_PATTERN.sub(repl, self.text, count=1)
        if not replaced:
            if _GEO_PKG_NOOPTS_PATTERN.search(new_text):
                new_text = _GEO_PKG_NOOPTS_PATTERN.sub(f"\\usepackage[margin={size}]{{geometry}}", new_text, count=1)
                replaced = True
            else:
                # Insert after documentclass
                m = _DOCCLASS_PATTERN.search(new_text)
                insert_at = m.end() if m else 0
                new_text = new_text[:insert_at] + f"\n\\usepackage[margin={size}]{{geometry}}\n" + new_text[insert_at:]
                replaced = True
        if new_text != self.text:
            return EditResult(True, f"Set geometry margin to {size}", new_text=new_text)
        return EditResult(False, "No change applied for margins")

    def _set_line_spacing(self, value: str) -> EditResult:
        # Support 1, 1.3, 1.5, 2
        # Ensure \usepackage{setspace} and inject appropriate command after \begin{document}
        text = self.text
        if "\\usepackage{setspace}" not in text:
            m = _DOCCLASS_PATTERN.search(text)
            insert_at = m.end() if m else 0
            text = text[:insert_at] + "\n\\usepackage{setspace}\n" + text[insert_at:]
        
        # Prepare spacing command
        v = value.strip()
        if v in {"1", "1.0"}:
            spacing_cmd = "\\singlespacing"
        elif v in {"1.3", "1.25"}:
            spacing_cmd = "\\onehalfspacing"
        elif v in {"1.5"}:
            spacing_cmd = "\\onehalfspacing"
        elif v in {"2", "2.0"}:
            spacing_cmd = "\\doublespacing"
        else:
            spacing_cmd = f"\\setstretch{{{value}}}"
        
        # Insert after \begin{document}
        begin_doc = re.search(r"\\begin\{document\}", text)
        if begin_doc:
            pos = begin_doc.end()
            text = text[:pos] + f"\n{spacing_cmd}\n" + text[pos:]
        else:
            text = spacing_cmd + "\n" + text
        
        if text != self.text:
            return EditResult(True, f"Applied line spacing {value}", new_text=text)
        return EditResult(False, "No change applied for line spacing")

    def _set_figure_width(self, width_expr: str) -> EditResult:
        # Rewrite includegraphics width option; preserves other options
        def replace_width(opts: Optional[str]) -> str:
            opts_map = self._options_to_map(opts or "", sep=",", kvsep="=")
            opts_map["width"] = width_expr
            return self._map_to_options(opts_map, sep=",", kvsep="=")

        changed = False
        def repl(m: re.Match) -> str:
            nonlocal changed
            opts = m.group("opts")
            file = m.group("file")
            new_opts = replace_width(opts)
            changed = True
            return f"\\includegraphics[{new_opts}]{{{file}}}"

        new_text = _INCLUDEGRAPHICS_PATTERN.sub(repl, self.text)
        if changed:
            return EditResult(True, f"Set includegraphics width to {width_expr}", new_text=new_text)
        return EditResult(False, "No includegraphics found to modify")

    def _set_documentclass(self, klass: str) -> EditResult:
        m = _DOCCLASS_PATTERN.search(self.text)
        if not m:
            return EditResult(False, "No documentclass found")
        opts = m.group("opts")
        new = f"\\documentclass[{opts}]{{{klass}}}" if opts else f"\\documentclass{{{klass}}}"
        new_text = self.text[:m.start()] + new + self.text[m.end():]
        if new_text != self.text:
            return EditResult(True, f"Changed documentclass to {klass}", new_text=new_text)
        return EditResult(False, "Documentclass unchanged")

    def _normalize_whitespace(self) -> EditResult:
        t = self.text
        # Collapse trailing spaces
        t = re.sub(r"[ \t]+$", "", t, flags=re.M)
        # Ensure single blank line around sections
        t = re.sub(r"\n{3,}", "\n\n", t)
        if t != self.text:
            return EditResult(True, "Normalized whitespace", new_text=t)
        return EditResult(False, "Whitespace already normalized")

    def _add_package(self, pkg: str) -> EditResult:
        r"""Add \\usepackage{pkg} to preamble if not already present."""
        # Parse package with optional options: "geometry[margin=1in]" or just "graphicx"
        if "[" in pkg:
            pkg_name = pkg[:pkg.index("[")].strip()
            pkg_opts = pkg[pkg.index("["):].strip()
            pkg_line = f"\\usepackage{pkg_opts}{{{pkg_name}}}"
        else:
            pkg_name = pkg.strip()
            pkg_line = f"\\usepackage{{{pkg_name}}}"
        
        # Check if already present
        if f"\\usepackage{{{pkg_name}}}" in self.text or f"\\usepackage[" in self.text and f"{{{pkg_name}}}" in self.text:
            return EditResult(False, f"Package {pkg_name} already loaded")
        
        # Insert after documentclass
        m = _DOCCLASS_PATTERN.search(self.text)
        if m:
            insert_at = m.end()
            new_text = self.text[:insert_at] + f"\n{pkg_line}\n" + self.text[insert_at:]
            return EditResult(True, f"Added package {pkg_name}", new_text=new_text)
        else:
            return EditResult(False, "No documentclass found; cannot determine preamble position")

    def _remove_package(self, pkg: str) -> EditResult:
        r"""Remove \\usepackage{pkg} from preamble."""
        pkg_name = pkg.strip()
        # Match with or without options
        pattern = re.compile(rf"\\usepackage(\[[^\]]*\])?\{{{re.escape(pkg_name)}\}}\n?", re.M)
        
        if not pattern.search(self.text):
            return EditResult(False, f"Package {pkg_name} not found")
        
        new_text = pattern.sub("", self.text)
        return EditResult(True, f"Removed package {pkg_name}", new_text=new_text)

    # Extractors
    def _extract_documentclass(self) -> Optional[str]:
        m = _DOCCLASS_PATTERN.search(self.text)
        return m.group("class") if m else None

    def _extract_geometry(self) -> Optional[str]:
        m = _GEO_PKG_PATTERN.search(self.text)
        if m:
            return m.group("opts")
        if _GEO_PKG_NOOPTS_PATTERN.search(self.text):
            return "(no options)"
        return None

    # Validation helpers
    def _validate_size(self, size: str) -> bool:
        """Check if size matches LaTeX dimension format (e.g., 1in, 2.5cm, 10mm)."""
        return bool(re.match(r"^\d+(\.\d+)?(in|cm|mm|pt|em|ex)$", size))

    def _validate_spacing(self, value: str) -> bool:
        """Check if spacing value is in valid range [1.0, 3.0]."""
        try:
            v = float(value)
            return 1.0 <= v <= 3.0
        except ValueError:
            return False

    def _validate_documentclass(self, klass: str) -> bool:
        """Check if documentclass is a known standard class."""
        known_classes = {
            "article", "report", "book", "letter", "beamer",
            "memoir", "scrartcl", "scrreprt", "scrbook",
            "IEEEtran", "proc", "minimal"
        }
        return klass.strip() in known_classes

    # Option helpers
    def _options_to_map(self, opts: str, sep: str = ",", kvsep: str = "=") -> Dict[str, str]:
        result: Dict[str, str] = {}
        for part in [o.strip() for o in opts.split(sep) if o.strip()]:
            if kvsep in part:
                k, v = part.split(kvsep, 1)
                result[k.strip()] = v.strip()
            else:
                result[part] = "true"
        return result

    def _map_to_options(self, m: Dict[str, str], sep: str = ",", kvsep: str = "=") -> str:
        items = []
        for k, v in m.items():
            if v == "true":
                items.append(k)
            else:
                items.append(f"{k}{kvsep}{v}")
        return sep.join(items)
