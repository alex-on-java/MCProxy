# Port Check Middleware - Implementation Plan

## Overview
The Port Check middleware provides dynamic tool visibility based on TCP port accessibility. This solves the problem where MCP servers (like chrome-devtools-mcp) expose tools even when the underlying service is not available, leading to tool call failures.

## Problem Statement
chrome-devtools-mcp always connects successfully and exposes tools regardless of whether Chrome is running on the specified port. When AI agents attempt to use these tools without Chrome available, the calls fail at runtime. This creates a poor user experience and wastes AI agent context.

## Key Findings

### chrome-devtools-mcp Behavior
- **Always connects**: The MCP server process starts successfully regardless of Chrome availability
- **Always exposes tools**: Tools appear in the tool list even when Chrome isn't running
- **Runtime failures**: Tool calls fail when Chrome is not accessible on the specified port

### Test Results
Testing with two configurations:
- **without-specifying-port**: Successfully connected to Chrome running on default port
- **with-browser-url localhost:9222**: Failed when Chrome was not running on port 9222

Example error when Chrome unavailable:
```
Failed to fetch browser webSocket URL from http://localhost:9222/json/version: fetch failed
```

## Solution Architecture

### Middleware Approach
Following MCProxy's existing middleware pattern, the solution uses **ClientMiddleware** that operates on individual server responses before aggregation.

**Why ClientMiddleware (not ProxyMiddleware)?**
- Operates per-server, allowing server-specific port checks
- Hooks into `modify_list_tools_result()` called on every `list_tools` request
- Aligns with existing architecture (tool_filter, security, logging)

### Design Decision: Pure Middleware Pattern
The implementation follows MCProxy's existing architectural pattern by placing all middleware configuration in `httpServer.middleware` rather than adding server-level configuration. This maintains architectural consistency.

## Implementation Status

### Completed
✅ **src/middleware/client_middleware/port_check.rs** - Full implementation with:
- `PortCheckClientMiddleware` struct
- TCP port accessibility check via `TcpStream::connect()`
- `modify_list_tools_result()` implementation that clears tools when port unavailable
- `PortCheckClientFactory` for configuration-based instantiation
- Comprehensive unit tests
- Configurable timeout (default: 100ms)

### Remaining Tasks
1. **Register middleware in registry.rs** - Add `PortCheckClientFactory` to built-in middleware
2. **Export module in client_middleware/mod.rs** - Add `pub mod port_check;`
3. **Update MIDDLEWARE.md** - Document the new middleware with examples

## Configuration Example

```json
{
  "mcpServers": {
    "chrome-devtools": {
      "command": "npx",
      "args": [
        "-y",
        "chrome-devtools-mcp@latest",
        "--browser-url",
        "http://localhost:9222"
      ]
    }
  },
  "httpServer": {
    "host": "127.0.0.1",
    "port": 8081,
    "middleware": {
      "client": {
        "servers": {
          "chrome-devtools": [
            {
              "type": "port_check",
              "enabled": true,
              "config": {
                "host": "localhost",
                "port": 9222,
                "timeout_ms": 100
              }
            }
          ]
        }
      }
    }
  }
}
```

## Configuration Options

### Required Fields
- **`host`** (string): Hostname or IP address to check
- **`port`** (number): TCP port to check (must be non-zero)

### Optional Fields
- **`timeout_ms`** (number): Connection timeout in milliseconds (default: 100)

## Expected Behavior

### When Port is NOT Accessible (Chrome not running)
1. Port check fails during `list_tools` call
2. All tools from this server are hidden (empty list returned)
3. Log message: `🔌 [server-name] Port localhost:9222 not accessible - hiding N tool(s)`

### When Port IS Accessible (Chrome running)
1. Port check succeeds during `list_tools` call
2. Tools are returned normally
3. Log message: `✅ [server-name] Port localhost:9222 accessible - exposing N tool(s)`

### Dynamic Behavior
- **Chrome starts**: Next `list_tools` call immediately shows tools
- **Chrome stops**: Next `list_tools` call immediately hides tools
- **No caching**: Port is checked on every `list_tools` request for real-time accuracy

## Performance Considerations
- Port check takes approximately 1-5ms per request
- Only executes during `list_tools` calls (not on every tool call)
- Configurable timeout prevents long waits
- Minimal overhead compared to overall request processing

## Manual Testing Procedure

### Prerequisites
- MCProxy project built and ready
- Chrome browser installed
- Chrome configured to run with remote debugging: `--remote-debugging-port=9222`

### Test Steps

#### 1. Prepare Environment
```bash
# Stop any running MCProxy instance
# Kill the process or press Ctrl+C if running in terminal

# Rebuild the project
cargo build --release
```

#### 2. Update Configuration
Edit `mcp_servers.json` to add port_check middleware:

```json
{
  "mcpServers": {
    "chrome-dev-tools-with-browser-url": {
      "type": "stdio",
      "command": "npx",
      "args": [
        "-y",
        "chrome-devtools-mcp@latest",
        "--browser-url",
        "http://localhost:9222"
      ],
      "env": {}
    }
  },
  "httpServer": {
    "host": "127.0.0.1",
    "port": 8081,
    "middleware": {
      "client": {
        "servers": {
          "chrome-dev-tools-with-browser-url": [
            {
              "type": "port_check",
              "enabled": true,
              "config": {
                "host": "localhost",
                "port": 9222
              }
            }
          ]
        }
      }
    }
  }
}
```

#### 3. Test Without Chrome Running
```bash
# Ensure Chrome is NOT running on port 9222
# Check with: lsof -i :9222 (should return nothing)

# Start MCProxy
./target/release/mcproxy mcp_servers.json

# In another terminal, test list_tools
curl -X POST http://127.0.0.1:8081/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'

# Expected: No chrome-devtools tools in the response
# Check logs for: "🔌 [chrome-dev-tools-with-browser-url] Port localhost:9222 not accessible"
```

#### 4. Verify No Regression with Other Servers
```bash
# Add another MCP server without port_check to the config
# Example: filesystem server

# Call list_tools again
curl -X POST http://127.0.0.1:8081/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'

# Expected: Other server tools should appear normally
```

#### 5. Test With Chrome Running
```bash
# Start Chrome with remote debugging
# macOS:
/Applications/Google\ Chrome.app/Contents/MacOS/Google\ Chrome \
  --remote-debugging-port=9222 &

# Linux:
google-chrome --remote-debugging-port=9222 &

# Windows:
"C:\Program Files\Google\Chrome\Application\chrome.exe" --remote-debugging-port=9222

# Verify port is open: lsof -i :9222

# Call list_tools again
curl -X POST http://127.0.0.1:8081/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'

# Expected: chrome-devtools tools should now appear
# Check logs for: "✅ [chrome-dev-tools-with-browser-url] Port localhost:9222 accessible"
```

#### 6. Test Dynamic Hide/Show
```bash
# Stop Chrome (kill the process)
killall "Google Chrome"

# Call list_tools again
curl -X POST http://127.0.0.1:8081/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'

# Expected: chrome-devtools tools should disappear again
# Check logs for: "🔌 [chrome-dev-tools-with-browser-url] Port localhost:9222 not accessible"
```

#### 7. Test Tool Functionality
```bash
# Start Chrome with remote debugging again
/Applications/Google\ Chrome.app/Contents/MacOS/Google\ Chrome \
  --remote-debugging-port=9222 &

# Call a chrome-devtools tool (example: list_pages)
curl -X POST http://127.0.0.1:8081/mcp \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc":"2.0",
    "id":1,
    "method":"tools/call",
    "params":{
      "name":"chrome-dev-tools-with-browser-url___list_pages",
      "arguments":{}
    }
  }'

# Expected: Tool should work and return page list
```

### Success Criteria
- ✅ No chrome-devtools tools visible when Chrome not running (port 9222 closed)
- ✅ Other MCP server tools remain visible (no regression)
- ✅ chrome-devtools tools appear when Chrome starts
- ✅ chrome-devtools tools disappear when Chrome stops
- ✅ Tools work correctly when exposed
- ✅ Appropriate log messages at each state change

### Troubleshooting
- **Tools still appear without Chrome**: Check middleware configuration syntax
- **Port check always fails**: Verify Chrome is running with `lsof -i :9222`
- **Build errors**: Ensure all remaining tasks are completed (registry.rs, mod.rs)
- **No logs appearing**: Check `RUST_LOG` environment variable (set to `info` or `debug`)

## Future Enhancements (Not in v1)
- **Caching**: Cache port check results with TTL to reduce overhead
- **Background monitoring**: Periodic background checks with state tracking
- **HTTP health checks**: Support `httpGet` probes in addition to TCP
- **Multiple probe types**: Support exec commands, gRPC health checks (k8s-style)
- **Startup behavior**: Option to skip server connection entirely if probe fails
- **Retry/backoff**: Automatic reconnection when service becomes available
