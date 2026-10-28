# Agent Guidance (NPM Launcher)

Cross-platform runner and installer for **Agent Guidance Rust MCP Server & GraphRAG Intelligence Engine**.

## Quick Start

Run instantly without installation:
```bash
npx -y agent-guidance --setup
```

Or install globally:
```bash
npm install -g agent-guidance
agent-guidance --setup
```

## Configure MCP Client

Add to your MCP client configuration (`claude_desktop_config.json`, `~/.agent-context/mcp.json`, etc.):

```json
{
  "mcpServers": {
    "agent-guidance": {
      "command": "npx",
      "args": ["-y", "agent-guidance"]
    }
  }
}
```

Or if installed globally:
```json
{
  "mcpServers": {
    "agent-guidance": {
      "command": "agent-guidance"
    }
  }
}
```

## Supported Platforms
- Windows (x64)
- macOS (Apple Silicon arm64 & Intel x64)
- Linux (x64)

Native prebuilt binaries are automatically fetched and cached from [GitHub Releases](https://github.com/JunMystery/Agent-Guidance/releases).
