from sql.parser import parse


def test_simple_select():
    sql = "SELECT id, name FROM users WHERE id = 123"
    ast = parse(sql)
    assert ast["type"] == "select"
    assert ast["columns"] == ["id", "name"]
    assert ast["from"] == "users"
    assert ast["where"]["left"] == "id"
    assert ast["where"]["op"] == "="
    assert ast["where"]["right"] == "123"


if __name__ == "__main__":
    test_simple_select()
    print("parser smoke test passed")
