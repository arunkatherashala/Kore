# Kore SQL Parser (scaffold)

This is an initial, lightweight SQL parser used by the Kore project as the foundation for the SQL engine.

Capabilities:
- Parse simple SELECT statements: `SELECT col1, col2 FROM table WHERE col1 = 5`
- Produces a small AST (dictionary) for downstream components.

Next steps:
- Expand expression parsing (AND/OR, parentheses)
- Add JOIN, GROUP BY, ORDER BY support
- Add tokenizer improvements and tests
