"""Format DataBlock results for display in Jupyter and console."""

from __future__ import annotations

from typing import Any, Sequence


def datablock_to_html(
    columns: Sequence[str],
    rows: Sequence[dict[str, Any]],
    max_rows: int = 50,
) -> str:
    """Generate a styled HTML table from column names and row dicts."""
    truncated = len(rows) > max_rows
    display_rows = rows[:max_rows]

    html_parts = [
        '<table style="border-collapse:collapse;font-family:monospace;font-size:13px;">',
        "<thead><tr>",
    ]
    for col in columns:
        html_parts.append(
            f'<th style="border:1px solid #ddd;padding:6px 10px;'
            f'background:#f4f4f4;text-align:left;">{_escape(col)}</th>'
        )
    html_parts.append("</tr></thead><tbody>")

    for i, row in enumerate(display_rows):
        bg = "#fff" if i % 2 == 0 else "#fafafa"
        html_parts.append(f'<tr style="background:{bg};">')
        for col in columns:
            val = row.get(col)
            cell = "NULL" if val is None else _escape(str(val))
            style = 'style="border:1px solid #ddd;padding:4px 10px;"'
            if val is None:
                style = 'style="border:1px solid #ddd;padding:4px 10px;color:#999;font-style:italic;"'
            html_parts.append(f"<td {style}>{cell}</td>")
        html_parts.append("</tr>")

    html_parts.append("</tbody></table>")

    if truncated:
        html_parts.append(
            f'<p style="color:#666;font-size:12px;">'
            f"Showing {max_rows} of {len(rows)} rows.</p>"
        )

    return "".join(html_parts)


def datablock_to_text(
    columns: Sequence[str],
    rows: Sequence[dict[str, Any]],
    max_rows: int = 20,
) -> str:
    """Generate a plain-text table (similar to spark.show())."""
    if not columns:
        return "(empty result)"

    display_rows = rows[:max_rows]
    truncated = len(rows) > max_rows

    col_widths = {c: len(c) for c in columns}
    str_rows: list[dict[str, str]] = []
    for row in display_rows:
        sr: dict[str, str] = {}
        for c in columns:
            val = row.get(c)
            s = "null" if val is None else str(val)
            sr[c] = s
            col_widths[c] = builtins_max(col_widths[c], len(s))
        str_rows.append(sr)

    sep = "+" + "+".join("-" * (col_widths[c] + 2) for c in columns) + "+"
    header = "|" + "|".join(f" {c:<{col_widths[c]}} " for c in columns) + "|"

    lines = [sep, header, sep]
    for sr in str_rows:
        line = "|" + "|".join(f" {sr[c]:<{col_widths[c]}} " for c in columns) + "|"
        lines.append(line)
    lines.append(sep)

    if truncated:
        lines.append(f"({max_rows} of {len(rows)} rows shown)")

    return "\n".join(lines)


def format_schema(columns: Sequence[str], dtypes: Sequence[tuple[str, str]]) -> str:
    """Format schema information for %schema command."""
    lines = ["root"]
    for name, dtype in dtypes:
        lines.append(f" |-- {name}: {dtype} (nullable = true)")
    return "\n".join(lines)


def suggest_chart(columns: Sequence[str], rows: Sequence[dict[str, Any]]) -> str | None:
    """Suggest a simple chart type for aggregate results.

    Returns an HTML snippet with a basic bar chart using inline SVG,
    or None if no chart suggestion is appropriate.
    """
    if not rows or len(columns) < 2:
        return None

    if len(rows) > 20 or len(rows) < 2:
        return None

    label_col = columns[0]
    value_col = columns[1]

    try:
        values = [float(row[value_col]) for row in rows]
    except (ValueError, TypeError):
        return None

    max_val = builtins_max(values) if values else 1
    if max_val == 0:
        max_val = 1

    bar_width = 300
    bar_height = 20
    gap = 4
    svg_height = (bar_height + gap) * len(rows) + 30
    svg_width = bar_width + 200

    bars = []
    for i, (row, val) in enumerate(zip(rows, values)):
        y = i * (bar_height + gap) + 25
        w = int((val / max_val) * bar_width)
        label = str(row[label_col])
        bars.append(
            f'<rect x="100" y="{y}" width="{w}" height="{bar_height}" '
            f'fill="#4a90d9" rx="3"/>'
            f'<text x="95" y="{y + 14}" text-anchor="end" '
            f'font-size="12" fill="#333">{_escape(label)}</text>'
            f'<text x="{100 + w + 5}" y="{y + 14}" '
            f'font-size="11" fill="#666">{val}</text>'
        )

    svg = (
        f'<svg width="{svg_width}" height="{svg_height}" '
        f'style="font-family:sans-serif;">'
        f'<text x="{svg_width // 2}" y="15" text-anchor="middle" '
        f'font-size="13" fill="#333">{_escape(value_col)} by {_escape(label_col)}</text>'
        + "".join(bars)
        + "</svg>"
    )
    return svg


def _escape(s: str) -> str:
    """Escape HTML special characters."""
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


builtins_max = max
