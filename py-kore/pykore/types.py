"""
PySpark-compatible data type definitions for KORE Engine.

Mirrors ``pyspark.sql.types`` so that existing PySpark code can switch to KORE
with minimal import changes.
"""

from __future__ import annotations


class DataType:
    """Base class for all KORE data types."""

    def simpleString(self) -> str:
        """Return a human-readable type name (e.g. ``"string"``)."""
        return self.typeName()

    def typeName(self) -> str:  # noqa: N802
        name = type(self).__name__
        return name[: -len("Type")].lower() if name.endswith("Type") else name.lower()

    def json(self) -> str:
        return f'"{self.typeName()}"'

    def __repr__(self) -> str:
        return f"{type(self).__name__}()"

    def __eq__(self, other: object) -> bool:
        return type(self) is type(other)

    def __hash__(self) -> int:
        return hash(type(self).__name__)


class NullType(DataType):
    pass


class StringType(DataType):
    pass


class BinaryType(DataType):
    pass


class BooleanType(DataType):
    pass


class DateType(DataType):
    pass


class TimestampType(DataType):
    pass


class DecimalType(DataType):
    def __init__(self, precision: int = 10, scale: int = 0):
        self.precision = precision
        self.scale = scale

    def simpleString(self) -> str:
        return f"decimal({self.precision},{self.scale})"

    def __repr__(self) -> str:
        return f"DecimalType({self.precision},{self.scale})"


class DoubleType(DataType):
    pass


class FloatType(DataType):
    pass


class ByteType(DataType):
    pass


class IntegerType(DataType):
    pass


class LongType(DataType):
    pass


class ShortType(DataType):
    pass


class ArrayType(DataType):
    """Array (list) of a single element type."""

    def __init__(self, elementType: DataType, containsNull: bool = True):
        self.elementType = elementType
        self.containsNull = containsNull

    def simpleString(self) -> str:
        return f"array<{self.elementType.simpleString()}>"

    def __repr__(self) -> str:
        return f"ArrayType({self.elementType!r}, {self.containsNull})"


class MapType(DataType):
    """Map (dict) from key type to value type."""

    def __init__(
        self,
        keyType: DataType,
        valueType: DataType,
        valueContainsNull: bool = True,
    ):
        self.keyType = keyType
        self.valueType = valueType
        self.valueContainsNull = valueContainsNull

    def simpleString(self) -> str:
        return f"map<{self.keyType.simpleString()},{self.valueType.simpleString()}>"

    def __repr__(self) -> str:
        return (
            f"MapType({self.keyType!r}, {self.valueType!r}, {self.valueContainsNull})"
        )


class StructField:
    """A single field inside a :class:`StructType`."""

    def __init__(
        self,
        name: str,
        dataType: DataType,
        nullable: bool = True,
        metadata: dict | None = None,
    ):
        self.name = name
        self.dataType = dataType
        self.nullable = nullable
        self.metadata = metadata or {}

    def simpleString(self) -> str:
        return f"{self.name}:{self.dataType.simpleString()}"

    def __repr__(self) -> str:
        return (
            f"StructField('{self.name}', {self.dataType!r}, {self.nullable})"
        )


class StructType(DataType):
    """Schema composed of a sequence of :class:`StructField` objects."""

    def __init__(self, fields: list[StructField] | None = None):
        self.fields = list(fields) if fields else []

    def add(
        self,
        field: str | StructField,
        data_type: DataType | str | None = None,
        nullable: bool = True,
        metadata: dict | None = None,
    ) -> "StructType":
        if isinstance(field, StructField):
            self.fields.append(field)
        else:
            dt = _parse_type(data_type) if isinstance(data_type, str) else (data_type or StringType())
            self.fields.append(StructField(field, dt, nullable, metadata))
        return self

    @property
    def names(self) -> list[str]:
        return [f.name for f in self.fields]

    def simpleString(self) -> str:
        inner = ",".join(f.simpleString() for f in self.fields)
        return f"struct<{inner}>"

    def __repr__(self) -> str:
        fields_repr = ", ".join(repr(f) for f in self.fields)
        return f"StructType([{fields_repr}])"

    def __iter__(self):
        return iter(self.fields)

    def __len__(self) -> int:
        return len(self.fields)

    def __getitem__(self, key):
        if isinstance(key, int):
            return self.fields[key]
        for f in self.fields:
            if f.name == key:
                return f
        raise KeyError(key)


_TYPE_MAP: dict[str, type[DataType]] = {
    "string": StringType,
    "binary": BinaryType,
    "boolean": BooleanType,
    "date": DateType,
    "timestamp": TimestampType,
    "double": DoubleType,
    "float": FloatType,
    "byte": ByteType,
    "tinyint": ByteType,
    "short": ShortType,
    "smallint": ShortType,
    "int": IntegerType,
    "integer": IntegerType,
    "long": LongType,
    "bigint": LongType,
}


def _parse_type(type_str: str | None) -> DataType:
    if type_str is None:
        return StringType()
    return _TYPE_MAP.get(type_str.lower(), StringType)()
