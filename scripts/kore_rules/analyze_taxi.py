"""Rule checks on the NYC TLC yellow taxi trip file (docs/kore_rule_mode.md). Read-only; prints JSON.

  python -I analyze_taxi.py FILE.parquet OUT.json

All money checks are in integer cents (round(x * 100)) so float noise is not counted as a violation.
"""
import json
import sys

import duckdb


def main():
    src, out = sys.argv[1], sys.argv[2]
    c = duckdb.connect()
    c.sql("SET autoinstall_known_extensions=false; SET autoload_known_extensions=false")
    c.sql(f"CREATE VIEW t AS SELECT * FROM read_parquet('{src.replace(chr(39), chr(39) * 2)}')")
    q = lambda s: c.sql(s).fetchall()
    res = {"rows": q("SELECT count(*) FROM t")[0][0]}
    res["nulls"] = {col: q(f'SELECT count(*) FROM t WHERE "{col}" IS NULL')[0][0] for (col,) in q("SELECT column_name FROM (DESCRIBE t)")}
    comps = ["fare_amount", "extra", "mta_tax", "tip_amount", "tolls_amount", "improvement_surcharge", "congestion_surcharge", "Airport_fee"]
    cents = lambda e: f"CAST(round(coalesce({e}, 0) * 100) AS BIGINT)"
    full = " + ".join(cents(f'"{x}"') for x in comps)
    res["total_eq_all8_cents"] = q(f'SELECT count(*) FROM t WHERE {cents("total_amount")} = {full}')[0][0]
    # each component left out (does a missing term explain the misses?)
    res["total_eq_without"] = {}
    for x in comps:
        rest = " + ".join(cents(f'"{y}"') for y in comps if y != x)
        res["total_eq_without"][x] = q(f'SELECT count(*) FROM t WHERE {cents("total_amount")} = {rest}')[0][0]
    # misses of the full formula: size of the difference
    res["total_miss_diff_cents_top"] = q(f'SELECT {cents("total_amount")} - ({full}) AS d, count(*) n FROM t GROUP BY d ORDER BY n DESC LIMIT 12')
    res["total_eq_all8_float_exact"] = q("SELECT count(*) FROM t WHERE total_amount = " + " + ".join(f'coalesce("{x}", 0)' for x in comps))[0][0]
    res["dropoff_lt_pickup"] = q("SELECT count(*) FROM t WHERE tpep_dropoff_datetime < tpep_pickup_datetime")[0][0]
    res["dropoff_eq_pickup"] = q("SELECT count(*) FROM t WHERE tpep_dropoff_datetime = tpep_pickup_datetime")[0][0]
    res["pickup_outside_jan2024"] = q("SELECT count(*) FROM t WHERE tpep_pickup_datetime < TIMESTAMP '2024-01-01' OR tpep_pickup_datetime >= TIMESTAMP '2024-02-01'")[0][0]
    res["dropoff_outside_jan2024"] = q("SELECT count(*) FROM t WHERE tpep_dropoff_datetime < TIMESTAMP '2024-01-01' OR tpep_dropoff_datetime >= TIMESTAMP '2024-02-01'")[0][0]
    res["trip_seconds_quantiles"] = q("SELECT quantile_cont(epoch(tpep_dropoff_datetime) - epoch(tpep_pickup_datetime), [0.01, 0.5, 0.99]) FROM t")[0][0]
    # RatecodeID -> fare_amount: rows per code and the share of the most common fare of that code
    res["ratecode_fare"] = q("""
        WITH f AS (SELECT RatecodeID, fare_amount, count(*) n FROM t GROUP BY ALL),
             g AS (SELECT RatecodeID, sum(n) nrows, max(n) top, arg_max(fare_amount, n) top_fare FROM f GROUP BY RatecodeID)
        SELECT RatecodeID, nrows, top_fare, top, round(100.0 * top / nrows, 2) FROM g ORDER BY nrows DESC""")
    res["negative_total"] = q("SELECT count(*) FROM t WHERE total_amount < 0")[0][0]
    json.dump(res, open(out, "w"), indent=1, default=str)
    print(json.dumps(res, indent=1, default=str))


if __name__ == "__main__":
    main()
