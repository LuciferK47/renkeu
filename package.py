#!/usr/bin/env python3
"""
Packaging script for LaTeX-AI distribution.

Excludes unnecessary directories:
- Frontend node_modules
- Python virtual environments
- Build artifacts
- Cache directories
- Test artifacts

Usage:
    python package.py [output_name]

Example:
    python package.py latex-ai-v0.1.0.zip
"""

import os
import shutil
import zipfile
import argparse
from pathlib import Path


# Directories and patterns to exclude
EXCLUDE_DIRS = {
    # Python virtual environments
    '.venv',
    '.env',
    'venv',
    'env',
    '.virtualenv',
    
    # Frontend build artifacts and dependencies
    'frontend/node_modules',
    'frontend/.vite',
    'frontend/dist',
    'node_modules',  # Root-level node_modules if present
    
    # Python build artifacts
    '__pycache__',
    '.pytest_cache',
    '.mypy_cache',
    'dist',
    'build',
    '*.egg-info',
    '.tox',
    '.nox',
    
    # Version control and IDE
    '.git',
    '.github',
    '.vscode',
    '.idea',
    '.vs',
    
    # Compilation artifacts
    '.compile_cache',
    'latex_ai.egg-info',
    
    # Test and coverage
    'htmlcov',
    '.coverage',
}

EXCLUDE_FILES = {
    '.gitignore',
    '.env.local',
    '.env.*.local',
    '*.pyc',
    '*.pyo',
    '*.pyd',
    '.DS_Store',
    'Thumbs.db',
    '.pytest_cache',
    '.coverage',
    '*.log',
}


def should_exclude(path: Path, root: Path) -> bool:
    """Check if a path should be excluded from the package.
    
    Args:
        path: Path to check
        root: Root directory of the project
        
    Returns:
        True if path should be excluded
    """
    # Get relative path from root
    try:
        rel_path = path.relative_to(root)
    except ValueError:
        return False
    
    # Check directory exclusions
    for exclude_dir in EXCLUDE_DIRS:
        if exclude_dir.endswith('/*'):
            # Pattern like "node_modules/*"
            parent = Path(exclude_dir.rstrip('/*'))
            if parent in rel_path.parents or rel_path.parent == parent:
                return True
        else:
            # Direct match or parent match
            if rel_path.name == exclude_dir:
                return True
            if any(part == exclude_dir for part in rel_path.parts):
                return True
    
    # Check file exclusions
    for exclude_file in EXCLUDE_FILES:
        if '*' in exclude_file:
            # Pattern match (simple glob)
            import fnmatch
            if fnmatch.fnmatch(rel_path.name, exclude_file):
                return True
        else:
            if rel_path.name == exclude_file:
                return True
    
    return False


def create_package(output_path: str, root: Path = None):
    """Create a clean distribution package.
    
    Args:
        output_path: Where to save the ZIP file
        root: Root directory (defaults to current directory)
    """
    if root is None:
        root = Path.cwd()
    else:
        root = Path(root)
    
    output_path = Path(output_path)
    total_files = 0
    excluded_count = 0
    
    print(f"[PACKING] Packaging LaTeX-AI from {root}")
    print(f"[OUTPUT] Output: {output_path}")
    print()
    
    with zipfile.ZipFile(output_path, 'w', zipfile.ZIP_DEFLATED) as zf:
        for file_path in root.rglob('*'):
            # Skip the output file itself
            if file_path.resolve() == output_path.resolve():
                continue
            
            # Check if should exclude
            if should_exclude(file_path, root):
                excluded_count += 1
                if file_path.is_dir():
                    print(f"  [EXCLUDED] {file_path.relative_to(root)}/ (directory)")
                continue
            
            # Add file to archive
            if file_path.is_file():
                arcname = file_path.relative_to(root)
                zf.write(file_path, arcname=arcname)
                total_files += 1
    
    output_size_mb = output_path.stat().st_size / (1024 * 1024)
    print()
    print(f"[SUCCESS] Package created successfully!")
    print(f"   Files included: {total_files}")
    print(f"   Directories/files excluded: {excluded_count}")
    print(f"   Size: {output_size_mb:.1f} MB")
    print()
    print(f"   Location: {output_path.absolute()}")


def main():
    parser = argparse.ArgumentParser(
        description='Package LaTeX-AI without node_modules, .venv, and build artifacts'
    )
    parser.add_argument(
        'output',
        nargs='?',
        default='latex-ai-distribution.zip',
        help='Output ZIP file name (default: latex-ai-distribution.zip)'
    )
    parser.add_argument(
        '--root',
        default=None,
        help='Root directory of LaTeX-AI project (default: current directory)'
    )
    
    args = parser.parse_args()
    
    try:
        create_package(args.output, args.root)
    except Exception as e:
        print(f"[ERROR] Error: {e}")
        return 1
    
    return 0


if __name__ == '__main__':
    exit(main())
