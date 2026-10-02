"""TPC-H-shaped data generator (NOT the official dbgen).

Follows the schema, key relationships and the value rules the 22 queries depend on (supplier sets per part,
one third of customers without orders, ship/commit/receipt date offsets, return/line status rules, brand/type/container
vocabularies, phone country codes, comment phrases for Q13/Q16). Distributions are close to dbgen but not identical,
so absolute row counts of query results differ from published answers. Both engines read exactly the same files.

Dates are ISO strings in every engine so that no engine gets a native-date advantage.

    python gen_data.py --sf 0.1 --out data/sf0.1
"""
import argparse
import os

import numpy as np
import pyarrow as pa
import pyarrow.csv as pacsv
import pyarrow.parquet as pq

REGIONS = ["AFRICA", "AMERICA", "ASIA", "EUROPE", "MIDDLE EAST"]
NATIONS = [("ALGERIA", 0), ("ARGENTINA", 1), ("BRAZIL", 1), ("CANADA", 1), ("EGYPT", 4), ("ETHIOPIA", 0),
           ("FRANCE", 3), ("GERMANY", 3), ("INDIA", 2), ("INDONESIA", 2), ("IRAN", 4), ("IRAQ", 4),
           ("JAPAN", 2), ("JORDAN", 4), ("KENYA", 0), ("MOROCCO", 0), ("MOZAMBIQUE", 0), ("PERU", 1),
           ("CHINA", 2), ("ROMANIA", 3), ("SAUDI ARABIA", 4), ("VIETNAM", 2), ("RUSSIA", 3),
           ("UNITED KINGDOM", 3), ("UNITED STATES", 1)]
COLORS = ("almond antique aquamarine azure beige bisque black blanched blue blush brown burlywood burnished chartreuse "
          "chiffon chocolate coral cornflower cornsilk cream cyan dark deep dim dodger drab firebrick floral forest "
          "frosted gainsboro ghost goldenrod green grey honeydew hot indian ivory khaki lace lavender lawn lemon light "
          "lime linen magenta maroon medium metallic midnight mint misty moccasin navajo navy olive orange orchid pale "
          "papaya peach peru pink plum powder puff purple red rose rosy royal saddle salmon sandy seashell sienna sky "
          "slate smoke snow spring steel tan thistle tomato turquoise violet wheat white yellow").split()
TYPE1 = ["STANDARD", "SMALL", "MEDIUM", "LARGE", "ECONOMY", "PROMO"]
TYPE2 = ["ANODIZED", "BURNISHED", "PLATED", "POLISHED", "BRUSHED"]
TYPE3 = ["TIN", "NICKEL", "BRASS", "STEEL", "COPPER"]
CONT1 = ["SM", "LG", "MED", "JUMBO", "WRAP"]
CONT2 = ["CASE", "BOX", "BAG", "JAR", "PKG", "PACK", "CAN", "DRUM"]
SEGMENTS = ["AUTOMOBILE", "BUILDING", "FURNITURE", "MACHINERY", "HOUSEHOLD"]
PRIORITIES = ["1-URGENT", "2-HIGH", "3-MEDIUM", "4-NOT SPECIFIED", "5-LOW"]
INSTRUCT = ["DELIVER IN PERSON", "COLLECT COD", "NONE", "TAKE BACK RETURN"]
MODES = ["REG AIR", "AIR", "RAIL", "SHIP", "TRUCK", "MAIL", "FOB"]
WORDS = ("furiously sleep carefully regular final ironic pending bold express quick even blithe ruthless slyly quietly "
         "deposits accounts packages requests instructions theodolites foxes pinto beans platelets dependencies "
         "excuses ideas dolphins tithes courts patterns").split()
EPOCH = np.datetime64("1992-01-01")


def iso(days):
    return (EPOCH + days.astype("timedelta64[D]")).astype("datetime64[D]").astype(str)


def pick(rng, choices, n):
    return np.array(choices)[rng.integers(0, len(choices), n)]


def comments(rng, n, special=None, rate=0.0):
    w = np.array(WORDS)
    out = np.char.add(np.char.add(w[rng.integers(0, len(w), n)], " "), w[rng.integers(0, len(w), n)])
    if special:
        hit = rng.random(n) < rate
        out = np.where(hit, np.char.add(np.char.add(out, " "), special), out)
    return out


def money(x):
    return np.round(x, 2)


def generate(sf, seed=19):
    rng = np.random.default_rng(seed)
    n_sup = max(int(10_000 * sf), 10)
    n_part = max(int(200_000 * sf), 40)
    n_cust = max(int(150_000 * sf), 30)
    n_ord = max(int(1_500_000 * sf), 300)
    t = {}

    t["region"] = pa.table({"r_regionkey": np.arange(5), "r_name": REGIONS, "r_comment": comments(rng, 5)})
    t["nation"] = pa.table({"n_nationkey": np.arange(25), "n_name": [n for n, _ in NATIONS],
                            "n_regionkey": [r for _, r in NATIONS], "n_comment": comments(rng, 25)})

    sk = np.arange(1, n_sup + 1)
    s_nat = rng.integers(0, 25, n_sup)
    t["supplier"] = pa.table({
        "s_suppkey": sk, "s_name": np.char.add("Supplier#", np.char.zfill(sk.astype(str), 9)),
        "s_address": comments(rng, n_sup), "s_nationkey": s_nat,
        "s_phone": np.char.add(np.char.add((s_nat + 10).astype(str), "-"), rng.integers(100, 999, n_sup).astype(str)),
        "s_acctbal": money(rng.uniform(-999.99, 9999.99, n_sup)),
        "s_comment": comments(rng, n_sup, "Customer Complaints", 0.005)})

    pk = np.arange(1, n_part + 1)
    words = np.array(COLORS)
    pname = words[rng.integers(0, len(words), n_part)]
    for _ in range(4):
        pname = np.char.add(np.char.add(pname, " "), words[rng.integers(0, len(words), n_part)])
    mfgr = rng.integers(1, 6, n_part)
    retail = money((90000 + (pk // 10) % 20001 + 100 * (pk % 1000)) / 100.0)
    t["part"] = pa.table({
        "p_partkey": pk, "p_name": pname, "p_mfgr": np.char.add("Manufacturer#", mfgr.astype(str)),
        "p_brand": np.char.add(np.char.add("Brand#", mfgr.astype(str)), rng.integers(1, 6, n_part).astype(str)),
        "p_type": np.char.add(np.char.add(np.char.add(pick(rng, TYPE1, n_part), " "), np.char.add(pick(rng, TYPE2, n_part), " ")),
                              pick(rng, TYPE3, n_part)),
        "p_size": rng.integers(1, 51, n_part),
        "p_container": np.char.add(np.char.add(pick(rng, CONT1, n_part), " "), pick(rng, CONT2, n_part)),
        "p_retailprice": retail, "p_comment": comments(rng, n_part)})

    ps_part = np.repeat(pk, 4)
    i = np.tile(np.arange(4), n_part)
    ps_supp = (ps_part + i * (n_sup // 4 + (ps_part - 1) // n_sup)) % n_sup + 1
    t["partsupp"] = pa.table({
        "ps_partkey": ps_part, "ps_suppkey": ps_supp, "ps_availqty": rng.integers(1, 10000, len(ps_part)),
        "ps_supplycost": money(rng.uniform(1.0, 1000.0, len(ps_part))), "ps_comment": comments(rng, len(ps_part))})

    ck = np.arange(1, n_cust + 1)
    c_nat = rng.integers(0, 25, n_cust)
    t["customer"] = pa.table({
        "c_custkey": ck, "c_name": np.char.add("Customer#", np.char.zfill(ck.astype(str), 9)),
        "c_address": comments(rng, n_cust), "c_nationkey": c_nat,
        "c_phone": np.char.add(np.char.add(np.char.add((c_nat + 10).astype(str), "-"), rng.integers(100, 999, n_cust).astype(str)),
                               np.char.add("-", rng.integers(1000, 9999, n_cust).astype(str))),
        "c_acctbal": money(rng.uniform(-999.99, 9999.99, n_cust)), "c_mktsegment": pick(rng, SEGMENTS, n_cust),
        "c_comment": comments(rng, n_cust)})

    # orders: a third of the customers never order
    ordering = ck[ck % 3 != 0]
    ok = np.arange(1, n_ord + 1)
    o_cust = ordering[rng.integers(0, len(ordering), n_ord)]
    o_day = rng.integers(0, (np.datetime64("1998-08-02") - EPOCH).astype(int) + 1, n_ord)
    n_lines = rng.integers(1, 8, n_ord)
    total_lines = int(n_lines.sum())

    li_order = np.repeat(ok, n_lines)
    li_day0 = np.repeat(o_day, n_lines)
    li_num = np.concatenate([np.arange(1, k + 1) for k in n_lines])
    li_part = rng.integers(1, n_part + 1, total_lines)
    j = rng.integers(0, 4, total_lines)
    li_supp = (li_part + j * (n_sup // 4 + (li_part - 1) // n_sup)) % n_sup + 1
    qty = rng.integers(1, 51, total_lines)
    ship = li_day0 + rng.integers(1, 122, total_lines)
    commit = li_day0 + rng.integers(30, 91, total_lines)
    receipt = ship + rng.integers(1, 31, total_lines)
    cutoff = (np.datetime64("1995-06-17") - EPOCH).astype(int)
    rflag = np.where(receipt <= cutoff, np.where(rng.random(total_lines) < 0.5, "R", "A"), "N")
    lstatus = np.where(ship > cutoff, "O", "F")
    ext = money(qty * retail[li_part - 1])
    disc = rng.integers(0, 11, total_lines) / 100.0
    tax = rng.integers(0, 9, total_lines) / 100.0
    t["lineitem"] = pa.table({
        "l_orderkey": li_order, "l_partkey": li_part, "l_suppkey": li_supp, "l_linenumber": li_num,
        "l_quantity": qty.astype(float), "l_extendedprice": ext, "l_discount": disc, "l_tax": tax,
        "l_returnflag": rflag, "l_linestatus": lstatus, "l_shipdate": iso(ship), "l_commitdate": iso(commit),
        "l_receiptdate": iso(receipt), "l_shipinstruct": pick(rng, INSTRUCT, total_lines),
        "l_shipmode": pick(rng, MODES, total_lines), "l_comment": comments(rng, total_lines)})

    line_total = ext * (1 + tax) * (1 - disc)
    o_total = money(np.bincount(li_order - 1, weights=line_total, minlength=n_ord))
    f_lines = np.bincount(li_order - 1, weights=(lstatus == "F").astype(int), minlength=n_ord)
    status = np.where(f_lines == n_lines, "F", np.where(f_lines == 0, "O", "P"))
    t["orders"] = pa.table({
        "o_orderkey": ok, "o_custkey": o_cust, "o_orderstatus": status, "o_totalprice": o_total,
        "o_orderdate": iso(o_day), "o_orderpriority": pick(rng, PRIORITIES, n_ord),
        "o_clerk": np.char.add("Clerk#", np.char.zfill(rng.integers(1, max(int(1000 * sf), 2) + 1, n_ord).astype(str), 9)),
        "o_shippriority": np.zeros(n_ord, dtype=np.int64),
        "o_comment": comments(rng, n_ord, "special requests", 0.01)})
    return t


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--sf", type=float, default=0.1)
    ap.add_argument("--out", default=None)
    a = ap.parse_args()
    out = a.out or os.path.join(os.path.dirname(os.path.abspath(__file__)), "data", f"sf{a.sf}")
    os.makedirs(out, exist_ok=True)
    for name, tab in generate(a.sf).items():
        pacsv.write_csv(tab, os.path.join(out, f"{name}.csv"))
        pq.write_table(tab, os.path.join(out, f"{name}.parquet"), compression="zstd")
        print(f"{name:9} {tab.num_rows:>10,} rows")
    print("written to", out)


if __name__ == "__main__":
    main()
