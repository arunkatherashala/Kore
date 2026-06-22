"""Minimal recursive-descent SQL parser for SELECT statements.

Supports: SELECT <cols> FROM <table> [WHERE <expr>]
Produces a simple AST (dicts/lists) useful for starting the SQL engine.
"""
import re


class ParseError(Exception):
    pass


class SQLParser:
    TOKENS = [
        (r"\s+", None),
        (r",", "COMMA"),
        (r"\*", "STAR"),
        (r"\(", "LPAREN"),
        (r"\)", "RPAREN"),
        (r"=|<>|!=|<=|>=|<|>", "OP"),
        (r"\bSELECT\b", "SELECT"),
        (r"\bFROM\b", "FROM"),
        (r"\bWHERE\b", "WHERE"),
        (r"[A-Za-z_][A-Za-z0-9_\.]*", "IDENT"),
        (r"'[^']*'", "STRING"),
        (r"[0-9]+(?:\.[0-9]+)?", "NUMBER"),
        (r"\.", "DOT"),
    ]

    def __init__(self, sql: str):
        self.sql = sql
        self.tokens = self._tokenize(sql)
        self.pos = 0

    def _tokenize(self, s):
        pattern = re.compile("|".join(f"(?P<T{n}>{p})" for n, (_, p) in enumerate([(p, n) for p, n in []]) ) )
        # build pattern correctly
        parts = []
        for idx, (pat, name) in enumerate(self.TOKENS):
            parts.append(f"(?P<T{idx}>{pat})")
        regex = re.compile("|".join(parts), re.IGNORECASE)
        pos = 0
        toks = []
        while pos < len(s):
            m = regex.match(s, pos)
            if not m:
                raise ParseError(f"Unexpected input at: {s[pos:pos+40]!r}")
            for idx, (pat, name) in enumerate(self.TOKENS):
                if m.group(f"T{idx}"):
                    if name:
                        toks.append((name, m.group(f"T{idx}")))
                    break
            pos = m.end()
        toks.append(("EOF", ""))
        return toks

    def _peek(self):
        return self.tokens[self.pos]

    def _next(self):
        tok = self.tokens[self.pos]
        self.pos += 1
        return tok

    def _expect(self, ttype):
        tok = self._peek()
        if tok[0] != ttype:
            raise ParseError(f"Expected {ttype} but got {tok}")
        return self._next()

    def parse(self):
        ast = self.parse_select()
        if self._peek()[0] != "EOF":
            raise ParseError("Unexpected tokens after end of statement")
        return ast

    def parse_select(self):
        self._expect("SELECT")
        cols = self.parse_columns()
        self._expect("FROM")
        table = self.parse_table()
        where = None
        if self._peek()[0] == "WHERE":
            self._next()
            where = self.parse_expression()
        return {"type": "select", "columns": cols, "from": table, "where": where}

    def parse_columns(self):
        cols = []
        if self._peek()[0] == "STAR":
            self._next()
            return ["*"]
        while True:
            tok = self._expect("IDENT")
            cols.append(tok[1])
            if self._peek()[0] == "COMMA":
                self._next()
                continue
            break
        return cols

    def parse_table(self):
        tok = self._expect("IDENT")
        return tok[1]

    def parse_expression(self):
        # Very small expression: IDENT OP (IDENT|NUMBER|STRING)
        left = self._expect("IDENT")[1]
        op = self._expect("OP")[1]
        right_type, right_val = self._next()
        if right_type not in ("IDENT", "NUMBER", "STRING"):
            raise ParseError("Invalid right-hand side in expression")
        return {"left": left, "op": op, "right": right_val}


def parse(sql: str):
    p = SQLParser(sql)
    return p.parse()
