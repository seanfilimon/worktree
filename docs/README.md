# Worktree Documentation

Welcome to the Worktree documentation! This directory contains comprehensive guides, references, and specifications for the Worktree version control system and its components.

## Table of Contents

### Getting Started

- **[Quick Start Guide](../README.md)** - Get up and running with Worktree in minutes
- **[Installation](../README.md#installation)** - Installation instructions for all platforms

### Core Documentation

- **[Server Architecture](server-architecture.md)** - Understanding the Worktree server design and internals
- **[Protocol Specification](protocol-spec.md)** - Complete protocol specification for client-server communication
- **[Git Compatibility](git-compatibility.md)** - How Worktree integrates with and differs from Git

### User Guides

- **[CLI Reference](cli-reference.md)** - Complete command-line interface reference
- **[SDK Guide](sdk-guide.md)** - Using the Worktree SDK in your applications
- **[Admin Panel](admin-panel.md)** - Managing and monitoring Worktree servers

### Components

#### Worktree Server
The background daemon that manages version control operations.

- Architecture overview
- Configuration options
- Performance tuning
- Deployment strategies

#### Worktree CLI
Command-line interface for interacting with repositories.

- Command reference
- Usage examples
- Configuration
- Scripting and automation

#### Worktree Admin Panel
Web-based management interface for server monitoring and control.

- Setup and configuration
- API reference
- Authentication
- Monitoring and metrics

#### Worktree SDK
Library for building custom integrations and tools.

- API documentation
- Code examples
- Best practices
- Integration patterns

### Advanced Topics

#### Protocol Details

The Worktree protocol enables efficient communication between clients and servers:

- Binary protocol format
- Message types and structure
- Streaming and chunked transfers
- Authentication mechanisms
- Error handling

#### Server Architecture

Deep dive into the server's internal design:

- Multi-threaded architecture
- Repository management
- File watching and change detection
- Caching strategies
- Performance optimization

#### Git Integration

How Worktree works alongside Git:

- Repository structure compatibility
- Git interoperability
- Migration strategies
- Hybrid workflows

### API Reference

#### REST API (Admin Panel)

- Authentication
- Endpoints
  - Health & Status
  - Server Control
  - Repository Management
  - Statistics
  - Maintenance
- Response formats
- Error codes

#### SDK API

- Initialization and connection
- Repository operations
- Commit and branch management
- File operations
- Event handling

### Development

#### Building from Source

```bash
git clone https://github.com/yourusername/worktree.git
cd worktree
cargo build --release
```

#### Running Tests

```bash
# Run all tests
cargo test

# Run specific component tests
cargo test -p worktree-server
cargo test -p worktree-admin
cargo test -p worktree-cli
```

#### Contributing

See the main [CONTRIBUTING.md](../CONTRIBUTING.md) for guidelines on:
- Code style
- Pull request process
- Issue reporting
- Development workflow

### Deployment

#### Production Deployment

- System requirements
- Installation methods
- Configuration best practices
- Security considerations
- Monitoring and logging
- Backup strategies

#### Docker Deployment

```bash
docker pull worktree/server:latest
docker run -d -p 8080:8080 worktree/server
```

#### Kubernetes Deployment

See [k8s-deployment.yaml](../examples/k8s-deployment.yaml) for example configurations.

### Configuration

#### Server Configuration

```toml
[server]
host = "0.0.0.0"
port = 9000
max_connections = 1000

[storage]
data_dir = "/var/lib/worktree"
cache_size_mb = 512

[git]
enabled = true
auto_fetch = true
```

#### Admin Panel Configuration

```toml
[server]
host = "127.0.0.1"
port = 8080

[worktree]
server_endpoint = "/tmp/worktree.sock"

[security]
auth_enabled = true
api_key = "your-secret-key"
```

### Troubleshooting

#### Common Issues

1. **Cannot connect to server**
   - Check if server is running
   - Verify socket/endpoint configuration
   - Check firewall rules

2. **Permission errors**
   - Verify file permissions
   - Check user/group ownership
   - Review SELinux/AppArmor policies

3. **Performance issues**
   - Enable debug logging
   - Check system resources
   - Review cache configuration
   - Analyze slow queries

#### Debug Mode

```bash
# Enable debug logging
RUST_LOG=debug worktree-server

# Trace-level logging for specific component
RUST_LOG=worktree_admin=trace worktree-admin
```

### Performance

#### Benchmarks

- Repository operations performance
- Concurrent connection handling
- Storage efficiency
- Network throughput

#### Optimization Tips

1. **Server-side**
   - Increase cache size for frequently accessed data
   - Enable compression for network transfers
   - Use SSD storage for better I/O performance
   - Tune worker thread count

2. **Client-side**
   - Batch operations when possible
   - Use incremental updates
   - Enable local caching
   - Optimize polling intervals

### Security

#### Best Practices

- **Authentication**: Always enable API key authentication in production
- **Encryption**: Use TLS/HTTPS for all network communication
- **Access Control**: Implement proper user permissions
- **Audit Logging**: Enable comprehensive audit trails
- **Updates**: Keep all components up to date

#### Security Considerations

- API key management
- TLS/SSL configuration
- Network security
- File system permissions
- Secrets management

### FAQ

#### General

**Q: What is Worktree?**
A: Worktree is a modern version control system with a client-server architecture, designed for efficient collaboration and integration.

**Q: How does Worktree differ from Git?**
A: Worktree provides a centralized server model while maintaining Git compatibility, offering real-time collaboration features and simplified workflows.

**Q: Can I use Worktree with existing Git repositories?**
A: Yes! Worktree is designed to work alongside Git repositories. See [Git Compatibility](git-compatibility.md) for details.

#### Technical

**Q: What programming language is Worktree written in?**
A: Worktree is written in Rust for performance, safety, and reliability.

**Q: What platforms are supported?**
A: Linux, macOS, and Windows are all supported.

**Q: Can Worktree handle large repositories?**
A: Yes, Worktree is designed to efficiently handle repositories of all sizes with optimized storage and caching.

### Resources

#### Links

- **Repository**: [https://github.com/yourusername/worktree](https://github.com/yourusername/worktree)
- **Issue Tracker**: [GitHub Issues](https://github.com/yourusername/worktree/issues)
- **Discussions**: [GitHub Discussions](https://github.com/yourusername/worktree/discussions)
- **Releases**: [GitHub Releases](https://github.com/yourusername/worktree/releases)

#### Community

- Discord: [Join our server](https://discord.gg/worktree)
- Twitter: [@worktree](https://twitter.com/worktree)
- Blog: [https://worktree.dev/blog](https://worktree.dev/blog)

#### Support

- Documentation: You're reading it!
- Examples: [../examples/](../examples/)
- Stack Overflow: Tag your questions with `worktree`

### License

Worktree is released under the MIT License. See [LICENSE](../LICENSE) for details.

### Changelog

See [CHANGELOG.md](../CHANGELOG.md) for version history and release notes.

## Documentation Structure

```
docs/
├── README.md                  # This file - documentation index
├── server-architecture.md     # Server design and internals
├── protocol-spec.md          # Protocol specification
├── git-compatibility.md      # Git integration guide
├── cli-reference.md          # CLI command reference
├── sdk-guide.md              # SDK usage guide
└── admin-panel.md            # Admin panel documentation
```

## Contributing to Documentation

We welcome contributions to improve our documentation! To contribute:

1. Fork the repository
2. Make your changes in the `docs/` directory
3. Ensure all links work correctly
4. Submit a pull request

Documentation should be:
- Clear and concise
- Well-organized with headers
- Include practical examples
- Keep code samples up to date
- Use proper markdown formatting

## Getting Help

If you can't find what you're looking for in the documentation:

1. Search the [issue tracker](https://github.com/yourusername/worktree/issues)
2. Check [discussions](https://github.com/yourusername/worktree/discussions)
3. Join our [Discord community](https://discord.gg/worktree)
4. Open a new issue with the `documentation` label

---

*Last updated: 2024*
*Worktree Version: 0.1.0*