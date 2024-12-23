# rip-zip
Handling cross-platform ZIP archives that just work everywhere

## Features

- Proper character encoding support (UTF-8 with EFS flag)
- Drag & drop support
- Works on Windows, macOS, and Linux
- Simple and lightweight
- Multiple directory support

## Installation

### Using Homebrew (macOS)
```bash
brew install hidari/tap/rip
```

### Binary Downloads (Windows/Linux)
Download the latest binary for your platform from the [releases page](https://github.com/hidari/rip-zip/releases).

#### Windows
1. Download `rip-zip-x86_64-pc-windows-msvc.zip`
2. Extract the ZIP file
3. Run `rip.exe` from the command line or use drag & drop

### From source
```bash
cargo install --git https://github.com/hidari/rip-zip
```

## Usage

### Command Line

```bash
# Create ZIP files from directories
rip directory1 directory2

# Show verbose output
rip -v directory1

# Show help
rip --help
```

### Drag & Drop

Simply drag and drop directories onto the `rip` executable. ZIP files will be created in the same location as the source directories.

## Building from source

```bash
cargo build --release
```

## Cross-Platform Compatibility Tests

To ensure file name compatibility between different platforms:

### Windows -> macOS/Linux
1. Create a ZIP on Windows including:
    - Files with Japanese characters (e.g., `テスト.txt`)
    - Files with special characters (`[テスト].txt`)
    - Long file names (over 100 characters)
    - Deep directory structures

2. Open the ZIP on macOS/Linux
    - All file names should display correctly
    - Directory structure should be preserved
    - Files should be extractable

### macOS/Linux -> Windows
1. Create a ZIP on macOS/Linux including:
    - Files with Japanese characters (e.g., `テスト.txt`)
    - Files using NFD Unicode normalization (common on macOS)
    - Files with characters normally invalid on Windows (replaced with '_')
    - Deep directory structures

2. Open the ZIP on Windows
    - All file names should display correctly
    - Directory structure should be preserved
    - Files should be extractable

### Known Platform Differences
- Windows has stricter file name restrictions
- macOS uses NFD Unicode normalization by default
- Path separators are automatically normalized
- Maximum path length varies by platform

## Security Considerations

This tool implements several security measures:

- Protection against path traversal attacks
- Symlink handling restrictions
- File name sanitization
- Resource usage limits (max file size: 1GB, total size: 4GB)
- No symlink following outside the source directory

Please be cautious when compressing untrusted files or directories.

## Technical Details

### Character Encoding
- All filenames are stored using UTF-8 encoding
- macOS-style NFD Unicode normalization is properly handled
- Conversion between different character encodings is automatic
- Invalid characters in filenames are replaced with '_'

### Path Handling
- Maximum path length
    - Windows: Limited to 260 characters by default
    - macOS/Linux: Virtually unlimited (>1000 characters)
    - Our limit: 100 levels deep for safety
- Path separators
    - Automatically normalized to forward slashes '/'
    - Windows backslashes '\' are converted automatically
- Special paths
    - Parent directory references ('..') are filtered out
    - Absolute paths are converted to relative
    - Symlinks are not followed for security

### Resource Limits
- Individual file size limit: 1GB
    - Prevents memory exhaustion
    - Suitable for most use cases
- Total ZIP size limit: 4GB
    - Ensures ZIP32 format compatibility
    - Prevents accidental huge archives

### Platform-Specific Notes
#### Windows
- Files with invalid Windows characters (e.g., `<>:"/\|?*`) are automatically renamed
- Long paths exceeding Windows limits are handled gracefully
- Japanese characters are fully supported (Shift-JIS compatible)

#### macOS
- NFD Unicode normalization is handled automatically
- Handles .DS_Store and resource fork files appropriately
- Full Japanese character support (UTF-8 native)

#### Linux
- Follows POSIX path conventions
- Handles various filesystem encodings
- Full Unicode support

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.