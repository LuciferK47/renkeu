"""Built-in LaTeX editing commands using the registry system."""
from __future__ import annotations
import re
from typing import Optional
from . import Command, CommandRegistry
from ..document import Document
from ..types import EditResult


@CommandRegistry.register
class SetMarginsCommand(Command):
    """Set page margins using the geometry package."""
    
    name = "set_margins"
    description = "Set page margins (e.g., 1in, 2.5cm, 10mm)"
    examples = [
        "set margins 1in",
        "set margins 2.5cm",
        "set margins 10mm"
    ]
    pattern = r"^set\s+margins?\s+(?P<size>\S+)$"
    
    def validate_args(self, **kwargs) -> Optional[str]:
        """Validate margin size format."""
        size = kwargs.get("size", "")
        if not re.match(r"^\d+(\.\d+)?(in|cm|mm|pt|em|ex)$", size):
            return f"Invalid size format: {size}. Use format like 1in, 2.5cm, 10mm"
        return None
    
    def execute(self, **kwargs) -> EditResult:
        size = kwargs["size"]
        
        # Validate
        if error := self.validate_args(**kwargs):
            return EditResult(False, error)
        
        # Use Document API for semantic edit
        new_text = self.document.update_package_option("geometry", "margin", size)
        
        # If geometry package doesn't exist, add it
        if new_text == self.document.text:
            new_text = self.document.add_package("geometry", f"margin={size}")
        
        if new_text != self.document.text:
            return EditResult(True, f"Set geometry margin to {size}", new_text=new_text)
        return EditResult(False, "No change applied for margins")


@CommandRegistry.register
class SetLineSpacingCommand(Command):
    """Set line spacing using the setspace package."""
    
    name = "set_line_spacing"
    description = "Set line spacing (1.0-3.0)"
    examples = [
        "set line spacing 1",
        "set line spacing 1.5",
        "set line spacing 2"
    ]
    pattern = r"^set\s+line\s+spacing\s+(?P<value>[\d.]+)$"
    
    def validate_args(self, **kwargs) -> Optional[str]:
        """Validate spacing value."""
        value = kwargs.get("value", "")
        try:
            v = float(value)
            if not (1.0 <= v <= 3.0):
                return f"Invalid spacing value: {value} (must be 1.0-3.0)"
        except ValueError:
            return f"Invalid spacing value: {value}"
        return None
    
    def execute(self, **kwargs) -> EditResult:
        value = kwargs["value"]
        
        # Validate
        if error := self.validate_args(**kwargs):
            return EditResult(False, error)
        
        # Ensure setspace package
        text = self.document.text
        if not self.document.has_package("setspace"):
            text = self.document.add_package("setspace")
        
        # Map value to LaTeX command
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
        
        # Create temporary document with updated text
        temp_doc = Document(self.document.path, text)
        new_text = temp_doc.insert_after_begin_document(spacing_cmd)
        
        if new_text != self.document.text:
            return EditResult(True, f"Applied line spacing {value}", new_text=new_text)
        return EditResult(False, "No change applied for line spacing")


@CommandRegistry.register
class SetFigureWidthCommand(Command):
    """Set width for all figures in the document."""
    
    name = "set_figure_width"
    description = "Set width for all \\includegraphics (e.g., 0.9\\textwidth)"
    examples = [
        "set figure width 0.8\\linewidth",
        "set figure width 0.9\\textwidth",
        "set figure width 10cm"
    ]
    pattern = r"^set\s+figure\s+width\s+(?P<width>.+)$"
    
    def execute(self, **kwargs) -> EditResult:
        width = kwargs["width"].strip()
        
        # Find and replace all includegraphics
        pattern = re.compile(
            r"\\includegraphics\s*(\[(?P<opts>[^\]]*)\])?\s*\{(?P<file>[^}]+)\}"
        )
        
        changed = False
        def replace_width(match: re.Match) -> str:
            nonlocal changed
            opts = match.group("opts") or ""
            file = match.group("file")
            
            # Parse options
            opts_dict = self.document._parse_options(opts)
            opts_dict["width"] = width
            new_opts = self.document._format_options(opts_dict)
            
            changed = True
            return f"\\includegraphics[{new_opts}]{{{file}}}"
        
        new_text = pattern.sub(replace_width, self.document.text)
        
        if changed:
            return EditResult(True, f"Set includegraphics width to {width}", new_text=new_text)
        return EditResult(False, "No includegraphics found to modify")


@CommandRegistry.register
class ChangeDocumentClassCommand(Command):
    """Change the document class."""
    
    name = "change_documentclass"
    description = "Change document class (article, report, book, etc.)"
    examples = [
        "change documentclass to article",
        "change documentclass to report",
        "change documentclass to book"
    ]
    pattern = r"^change\s+documentclass\s+to\s+(?P<classname>\w+)$"
    
    VALID_CLASSES = {
        "article", "report", "book", "letter", "beamer",
        "memoir", "scrartcl", "scrreprt", "scrbook",
        "IEEEtran", "proc", "minimal"
    }
    
    def validate_args(self, **kwargs) -> Optional[str]:
        """Validate document class name."""
        classname = kwargs.get("classname", "")
        if classname not in self.VALID_CLASSES:
            return f"Invalid documentclass: {classname}"
        return None
    
    def execute(self, **kwargs) -> EditResult:
        classname = kwargs["classname"]
        
        # Validate
        if error := self.validate_args(**kwargs):
            return EditResult(False, error)
        
        new_text = self.document.replace_documentclass(classname)
        
        if new_text != self.document.text:
            return EditResult(True, f"Changed documentclass to {classname}", new_text=new_text)
        return EditResult(False, "Documentclass unchanged")


@CommandRegistry.register
class NormalizeWhitespaceCommand(Command):
    """Remove trailing spaces and normalize newlines."""
    
    name = "normalize_whitespace"
    description = "Remove trailing spaces and collapse excessive newlines"
    examples = ["normalize whitespace"]
    pattern = r"^normalize\s+whitespace$"
    
    def execute(self, **kwargs) -> EditResult:
        t = self.document.text
        
        # Remove trailing spaces
        t = re.sub(r"[ \t]+$", "", t, flags=re.MULTILINE)
        
        # Collapse excessive newlines
        t = re.sub(r"\n{3,}", "\n\n", t)
        
        if t != self.document.text:
            return EditResult(True, "Normalized whitespace", new_text=t)
        return EditResult(False, "Whitespace already normalized")


@CommandRegistry.register
class AddPackageCommand(Command):
    """Add a LaTeX package to the preamble."""
    
    name = "add_package"
    description = "Add \\usepackage to preamble"
    examples = [
        "add package hyperref",
        "add package geometry[margin=1in]"
    ]
    pattern = r"^add\s+package\s+(?P<package>.+)$"
    
    def execute(self, **kwargs) -> EditResult:
        package = kwargs["package"].strip()
        
        # Parse package name and options
        if "[" in package:
            pkg_name = package[:package.index("[")].strip()
            opts = package[package.index("[") + 1:package.rindex("]")].strip()
        else:
            pkg_name = package
            opts = None
        
        # Check if already present
        if self.document.has_package(pkg_name):
            return EditResult(False, f"Package {pkg_name} already loaded")
        
        new_text = self.document.add_package(pkg_name, opts)
        return EditResult(True, f"Added package {pkg_name}", new_text=new_text)


@CommandRegistry.register
class RemovePackageCommand(Command):
    """Remove a LaTeX package from the preamble."""
    
    name = "remove_package"
    description = "Remove \\usepackage from preamble"
    examples = [
        "remove package graphicx",
        "remove package geometry"
    ]
    pattern = r"^remove\s+package\s+(?P<package>\w+)$"
    
    def execute(self, **kwargs) -> EditResult:
        package = kwargs["package"]
        
        if not self.document.has_package(package):
            return EditResult(False, f"Package {package} not found")
        
        new_text = self.document.remove_package(package)
        return EditResult(True, f"Removed package {package}", new_text=new_text)
