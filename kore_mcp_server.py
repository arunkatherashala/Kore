#!/usr/bin/env python3
"""
KORE MCP Server — AI agents can query .kore/.hkore files via Model Context Protocol.
World's first file format with native MCP support.

Usage:
  python kore_mcp_server.py

Tools exposed to AI:
  - kore_read: Read a .kore/.hkore file
  - kore_schema: Get schema/metadata
  - kore_query: Filter/aggregate data
  - kore_convert: Convert between formats
  - kore_write: Create .hkore files
"""
import sys, os, json
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), 'kore-python'))

try:
    from mcp.server import Server
    from mcp.server.stdio import stdio_server
    from mcp import types
    HAS_MCP = True
except ImportError:
    HAS_MCP = False

import kore_fileformat as kore


def block_to_dict(block, max_rows=100):
    result = {}
    for c in block.columns:
        data = list(c.data)[:max_rows]
        result[c.name] = data
    return {"rows": block.num_rows, "columns": block.num_columns, "data": result}


def schema_info(path):
    if path.endswith('.hkore'):
        header = kore.read_hybrid_header(path)
        return header
    b = kore.read_hybrid(path)
    return {"rows": b.num_rows, "columns": [c.name for c in b.columns]}


if HAS_MCP:
    server = Server("kore-fileformat")

    @server.list_tools()
    async def list_tools():
        return [
            types.Tool(
                name="kore_read",
                description="Read a .kore or .hkore file and return data as JSON. Supports column pruning.",
                inputSchema={
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Path to .kore or .hkore file"},
                        "columns": {"type": "array", "items": {"type": "string"}, "description": "Optional: specific columns to read"},
                        "max_rows": {"type": "integer", "description": "Max rows to return (default 100)", "default": 100}
                    },
                    "required": ["path"]
                }
            ),
            types.Tool(
                name="kore_schema",
                description="Get schema, metadata, and preview of a .kore/.hkore file without reading all data.",
                inputSchema={
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Path to .kore or .hkore file"}
                    },
                    "required": ["path"]
                }
            ),
            types.Tool(
                name="kore_query",
                description="Query a .hkore file with filter conditions. Returns matching rows.",
                inputSchema={
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Path to .hkore file"},
                        "filter": {"type": "string", "description": "Python expression for filtering, e.g. \"row['age'] > 30\""},
                        "columns": {"type": "array", "items": {"type": "string"}, "description": "Columns to return"},
                        "max_rows": {"type": "integer", "default": 100}
                    },
                    "required": ["path", "filter"]
                }
            ),
            types.Tool(
                name="kore_write",
                description="Create a .hkore file from JSON data.",
                inputSchema={
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Output .hkore file path"},
                        "data": {"type": "object", "description": "Column data as {col_name: [values]}"}
                    },
                    "required": ["path", "data"]
                }
            ),
            types.Tool(
                name="kore_convert",
                description="Convert between CSV, JSON, Parquet, and .hkore formats.",
                inputSchema={
                    "type": "object",
                    "properties": {
                        "src": {"type": "string", "description": "Source file path"},
                        "dst": {"type": "string", "description": "Destination file path"}
                    },
                    "required": ["src", "dst"]
                }
            ),
        ]

    @server.call_tool()
    async def call_tool(name, arguments):
        try:
            if name == "kore_read":
                path = arguments["path"]
                columns = arguments.get("columns")
                max_rows = arguments.get("max_rows", 100)
                block = kore.read_hybrid(path, columns=columns)
                result = block_to_dict(block, max_rows)
                return [types.TextContent(type="text", text=json.dumps(result, indent=2, default=str))]

            elif name == "kore_schema":
                path = arguments["path"]
                info = schema_info(path)
                if isinstance(info, str):
                    return [types.TextContent(type="text", text=info)]
                return [types.TextContent(type="text", text=json.dumps(info, indent=2))]

            elif name == "kore_query":
                path = arguments["path"]
                filter_expr = arguments["filter"]
                columns = arguments.get("columns")
                max_rows = arguments.get("max_rows", 100)
                block = kore.read_hybrid(path)
                keep = []
                for i in range(block.num_rows):
                    row = {c.name: c.data[i] for c in block.columns}
                    if eval(filter_expr, {"__builtins__": {}}, {"row": row}):
                        keep.append(i)
                        if len(keep) >= max_rows:
                            break
                result_block = kore.DataBlock()
                want = set(columns) if columns else None
                for c in block.columns:
                    if want and c.name not in want:
                        continue
                    filtered = [c.data[i] for i in keep]
                    result_block.add_column(c.name, c.dtype, filtered)
                result_block.num_rows = len(keep)
                return [types.TextContent(type="text", text=json.dumps(block_to_dict(result_block, max_rows), indent=2, default=str))]

            elif name == "kore_write":
                path = arguments["path"]
                data = arguments["data"]
                block = kore.DataBlock()
                for col_name, values in data.items():
                    if all(isinstance(v, int) for v in values):
                        block.add_column(col_name, kore.DataType.I64, values)
                    elif all(isinstance(v, (int, float)) for v in values):
                        block.add_column(col_name, kore.DataType.F64, [float(v) for v in values])
                    else:
                        block.add_column(col_name, kore.DataType.STR, [str(v) for v in values])
                kore.write_hybrid(path, block)
                return [types.TextContent(type="text", text=f"Written {block.num_rows} rows to {path}")]

            elif name == "kore_convert":
                import kore_convert as kc
                src, dst = arguments["src"], arguments["dst"]
                block = kc.read_any(src)
                kc.write_any(block, dst)
                return [types.TextContent(type="text", text=f"Converted {src} → {dst} ({block.num_rows} rows)")]

            else:
                return [types.TextContent(type="text", text=f"Unknown tool: {name}")]
        except Exception as e:
            return [types.TextContent(type="text", text=f"Error: {str(e)}")]

    async def main():
        async with stdio_server() as (read, write):
            await server.run(read, write, server.create_initialization_options())

    if __name__ == "__main__":
        import asyncio
        asyncio.run(main())

else:
    # Standalone mode without MCP SDK
    if __name__ == "__main__":
        print("KORE MCP Server — Standalone Mode")
        print("Install MCP SDK: pip install mcp")
        print()
        print("Available tools:")
        print("  kore_read   — Read .kore/.hkore files")
        print("  kore_schema — Get file schema/metadata")
        print("  kore_query  — Filter data with expressions")
        print("  kore_write  — Create .hkore files")
        print("  kore_convert — Convert formats")
        print()
        print("VS Code MCP config:")
        print(json.dumps({
            "mcpServers": {
                "kore-fileformat": {
                    "command": "python",
                    "args": [os.path.abspath(__file__)],
                    "env": {}
                }
            }
        }, indent=2))
