"""Safe patch application utilities using diff-match-patch.

Avoid naive unified-diff parsing that can corrupt files when context
lines are ambiguous or duplicated. This helper fails fast when hunks
cannot be applied cleanly.
"""
from __future__ import annotations

try:
    from diff_match_patch import diff_match_patch
    HAS_DIFF_MATCH = True
except ImportError:
    HAS_DIFF_MATCH = False


def _apply_unified_diff_strict(original_text: str, patch_text: str) -> str:
    """Fallback: strict unified-diff applier for when diff-match-patch is unavailable.
    
    This is a basic implementation that applies standard unified diff patches.
    It fails fast if hunks don't apply cleanly.
    
    Raises:
        ValueError: if any hunk fails to apply
    """
    from difflib import SequenceMatcher
    import re
    
    lines = original_text.splitlines(keepends=True)
    hunk_pattern = re.compile(r'^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@')
    
    patch_lines = patch_text.splitlines(keepends=True)
    line_idx = 0
    hunk_num = 0
    
    for patch_line in patch_lines:
        match = hunk_pattern.match(patch_line)
        if match:
            hunk_num += 1
            start_old = int(match.group(1)) - 1  # 1-indexed to 0-indexed
            count_old = int(match.group(2) or 1)
            start_new = int(match.group(3)) - 1
            count_new = int(match.group(4) or 1)
            
            # Read hunk content
            hunk_lines = []
            line_idx += 1
            while line_idx < len(patch_lines):
                hunk_line = patch_lines[line_idx]
                if hunk_pattern.match(hunk_line):
                    break
                if hunk_line.startswith(('---', '+++')):
                    line_idx += 1
                    break
                if hunk_line.startswith('-'):
                    hunk_lines.append(('del', hunk_line[1:]))
                elif hunk_line.startswith('+'):
                    hunk_lines.append(('add', hunk_line[1:]))
                elif hunk_line.startswith(' '):
                    hunk_lines.append(('ctx', hunk_line[1:]))
                else:
                    line_idx += 1
                    continue
                line_idx += 1
            
            # Apply hunk
            expected_lines = []
            new_lines = []
            for op, content in hunk_lines:
                if op in ('del', 'ctx'):
                    expected_lines.append(content)
                if op in ('add', 'ctx'):
                    new_lines.append(content)
            
            # Verify context matches
            actual = lines[start_old:start_old + count_old]
            actual_text = ''.join(actual)
            expected_text = ''.join(expected_lines)
            
            if actual_text != expected_text:
                raise ValueError(f"Patch hunk {hunk_num} failed: context mismatch at line {start_old + 1}")
            
            # Apply change
            lines[start_old:start_old + count_old] = new_lines
    
    return ''.join(lines)


def apply_semantic_patch(original_text: str, patch_text: str) -> str:
    """Apply a patch string safely using diff-match-patch (or strict fallback).

    Raises:
        ValueError: if any hunk fails to apply cleanly.
    """
    if HAS_DIFF_MATCH:
        dmp = diff_match_patch()
        patches = dmp.patch_fromText(patch_text)
        new_text, results = dmp.patch_apply(patches, original_text)

        if not all(results):
            failed_hunks = [i for i, ok in enumerate(results) if not ok]
            raise ValueError(f"Patch failed to apply cleanly on hunks: {failed_hunks}")

        return new_text
    else:
        # Fallback to strict unified-diff implementation
        return _apply_unified_diff_strict(original_text, patch_text)
