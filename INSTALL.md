# Installation Guide

## External Dependencies

Different commands require different external tools:

| Command | Required Tools |
|---------|---------------|
| `atomize` | verus-analyzer, scip |
| `list-functions` | None |
| `verify` | Verus (cargo verus) |

## Installing Dependencies

Use the [installers_for_various_tools](https://github.com/Beneficial-AI-Foundation/installers_for_various_tools) toolkit to install the required dependencies.

Refer to that repository's README for detailed installation instructions for:
- **Verus** (`verus_installer_from_release.py`) - required for `verify` command
- **Verus Analyzer** (`verus_analyzer_installer.py`) - required for `atomize` command
- **SCIP** (`scip_installer.py`) - required for `atomize` command

## Verifying Your Setup

```bash
# Check Verus (for verify command)
verus --version

# Check Verus Analyzer (for atomize command)
verus-analyzer --version

# Check SCIP (for atomize command)
scip --version
```
