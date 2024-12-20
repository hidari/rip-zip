# rip-zip
Handling cross-platform ZIP archives that just work everywhere

## Features

- Proper character encoding support (UTF-8 with EFS flag)
- Drag & drop support
- Works on Windows, macOS, and Linux
- Simple and lightweight
- Multiple directory support

## Installation

### From source

```bash
cargo install rip-zip
```

[//]: # (### Binary releases)

[//]: # ()
[//]: # (Download the latest release for your platform from the [releases page]&#40;https://github.com/yourusername/rip/releases&#41;.)

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

## Security Considerations

This tool implements several security measures:

- Protection against path traversal attacks
- Symlink handling restrictions
- File name sanitization
- Resource usage limits (max file size: 1GB, total size: 4GB)
- No symlink following outside the source directory

Please be cautious when compressing untrusted files or directories.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.