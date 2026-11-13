//! Deterministic structured machine data and random control data for rule mode (docs/kore_rule_mode.md).
//! Shared by `examples/bench_rules.rs` and `tests/rules.rs`.
#![allow(dead_code)]

use kore_core::{Column, DataBlock};

pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Self { Rng(seed) }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    pub fn below(&mut self, n: u64) -> u64 { self.next() % n }
    pub fn unit(&mut self) -> f64 { (self.next() >> 11) as f64 / (1u64 << 53) as f64 }
    /// approximately normal (sum of 4 uniforms, centred, unit variance)
    pub fn gauss(&mut self) -> f64 { (self.unit() + self.unit() + self.unit() + self.unit() - 2.0) * 1.732 }
}

/// `YYYY-MM-DDTHH:MM:SSZ` of epoch seconds (years 1970-9999).
pub fn iso(t: i64) -> String {
    let days = t.div_euclid(86_400);
    let rem = t.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 { y += 1; }
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

fn round_to(x: f64, s: i32) -> f64 { let p = 10f64.powi(s); (x * p).round() / p }

/// Sensor table: fixed-grid timestamps (ms, 1 s grid, jitter 0-19 ms), the same as text (s grid), a
/// temperature with drift + daily cycle + noise rounded to 2 decimals, a full-precision pressure with
/// drift and noise, a monotone counter, a battery level that steps down in long runs, a device id in
/// long sorted runs, and a reading that repeats with period 96 plus small noise.
pub fn sensors(n: usize, seed: u64) -> DataBlock {
    let mut r = Rng::new(seed);
    let (mut ts, mut ts_text, mut temp, mut press, mut cnt, mut batt, mut dev, mut cyc) =
        (vec![], vec![], vec![], vec![], vec![], vec![], vec![], vec![]);
    let t0 = 1_767_225_600i64; // 2026-01-01
    let mut c = 0i64;
    let mut b = 100i64;
    let pattern: Vec<i64> = (0..96).map(|k| 500 + ((k as f64 / 96.0 * 6.283).sin() * 200.0) as i64).collect();
    for i in 0..n {
        ts.push(Some((t0 + i as i64) * 1000 + r.below(20) as i64));
        ts_text.push(Some(iso(t0 + 60 * i as i64 + r.below(3) as i64)));
        let drift = i as f64 * 1e-5;
        temp.push(Some(round_to(20.0 + drift + 5.0 * (i as f64 * 6.283 / 1440.0).sin() + 0.05 * r.gauss(), 2)));
        press.push(Some(1013.25 + drift * 3.0 + 0.2 * r.gauss()));
        c += r.below(4) as i64;
        cnt.push(Some(c));
        if r.below(5000) == 0 && b > 1 { b -= 1; }
        batt.push(Some(b));
        dev.push(Some((i * 16 / n.max(1)) as i64 + 1000));
        cyc.push(Some(pattern[i % 96] + r.below(3) as i64));
    }
    DataBlock::new(vec![
        Column::int64("ts_ms", ts), Column::str_col("ts_text", ts_text), Column::float64("temp_c", temp),
        Column::float64("pressure", press), Column::int64("counter", cnt), Column::int64("battery", batt),
        Column::int64("device", dev), Column::int64("cycle", cyc),
    ]).unwrap()
}

const CITIES: [(&str, &str); 24] = [("Paris", "FR"), ("Lyon", "FR"), ("Nice", "FR"), ("Berlin", "DE"), ("Munich", "DE"), ("Hamburg", "DE"),
    ("Madrid", "ES"), ("Seville", "ES"), ("Rome", "IT"), ("Milan", "IT"), ("Turin", "IT"), ("Oslo", "NO"), ("Bergen", "NO"),
    ("Austin", "US"), ("Boston", "US"), ("Denver", "US"), ("Seattle", "US"), ("Toronto", "CA"), ("Ottawa", "CA"),
    ("Lima", "PE"), ("Quito", "EC"), ("Delhi", "IN"), ("Pune", "IN"), ("Osaka", "JP")];

/// Orders table: sequential ids, order time text, customer, city -> country (0.1 % exceptions), product
/// -> unit price (catalog), quantity, line total = round(qty * price, 2), cents columns with exact integer
/// relations (subtotal = qty * unit_cents, total = subtotal + shipping), and the previous order's city.
pub fn orders(n: usize, seed: u64) -> DataBlock {
    let mut r = Rng::new(seed);
    let catalog: Vec<i64> = (0..500).map(|_| 99 + r.below(20_000) as i64).collect();
    let mut cols: (Vec<Option<i64>>, Vec<Option<String>>, Vec<Option<i64>>, Vec<Option<String>>, Vec<Option<String>>) = (vec![], vec![], vec![], vec![], vec![]);
    let (mut prod, mut qty, mut price, mut total, mut ucents, mut sub, mut ship, mut tot_c, mut prev_city) =
        (vec![], vec![], vec![], vec![], vec![], vec![], vec![], vec![], vec![]);
    let mut t = 1_767_225_600i64;
    let mut last_city: Option<String> = None;
    for i in 0..n {
        cols.0.push(Some(100_000 + i as i64));
        t += r.below(40) as i64;
        cols.1.push(Some(iso(t)));
        cols.2.push(Some(r.below(50_000) as i64));
        let (city, country) = CITIES[(r.below(24) as usize * r.below(24) as usize) / 24];
        cols.3.push(Some(city.to_string()));
        cols.4.push(Some(if r.below(1000) == 0 { "XX".to_string() } else { country.to_string() }));
        prev_city.push(last_city.take());
        last_city = Some(city.to_string());
        let p = r.below(500) as usize;
        let q = 1 + r.below(9) as i64;
        let pr = catalog[p] as f64 / 100.0;
        prod.push(Some(p as i64));
        qty.push(Some(q));
        price.push(Some(pr));
        total.push(Some(round_to(q as f64 * pr, 2)));
        ucents.push(Some(catalog[p]));
        sub.push(Some(q * catalog[p]));
        let s = [0i64, 499, 999][r.below(3) as usize];
        ship.push(Some(s));
        tot_c.push(Some(q * catalog[p] + s));
    }
    DataBlock::new(vec![
        Column::int64("order_id", cols.0), Column::str_col("ordered_at", cols.1), Column::int64("customer", cols.2),
        Column::str_col("city", cols.3), Column::str_col("country", cols.4), Column::str_col("prev_city", prev_city),
        Column::int64("product", prod), Column::int64("qty", qty), Column::float64("unit_price", price),
        Column::float64("line_total", total), Column::int64("unit_cents", ucents), Column::int64("subtotal_cents", sub),
        Column::int64("shipping_cents", ship), Column::int64("total_cents", tot_c),
    ]).unwrap()
}

/// Server log: time text (1 s grid with gaps), service -> host (FD), level following a session state
/// machine (Markov structure), a message from 6 templates with numeric fields, latency, status, a request
/// id that increments.
pub fn logs(n: usize, seed: u64) -> DataBlock {
    let mut r = Rng::new(seed);
    let services = ["auth", "api", "billing", "search", "mail"];
    let hosts = ["auth-01", "api-03", "bill-02", "search-07", "mail-01"];
    let (mut ts, mut svc, mut host, mut level, mut msg, mut lat, mut status, mut rid) = (vec![], vec![], vec![], vec![], vec![], vec![], vec![], vec![]);
    let mut t = 1_767_225_600i64;
    let mut state = 0usize;
    // state machine over levels: INFO -> DEBUG/INFO, WARN -> ERROR/INFO, ERROR -> INFO
    let levels = ["INFO", "DEBUG", "WARN", "ERROR"];
    let next: [[u64; 4]; 4] = [[70, 20, 9, 1], [60, 38, 2, 0], [40, 0, 30, 30], [90, 0, 10, 0]];
    let mut req = 7_000_000i64;
    let mut job = 1200i64;
    for _ in 0..n {
        t += if r.below(10) == 0 { 2 + r.below(5) as i64 } else { 1 };
        ts.push(Some(iso(t)));
        let s = r.below(5) as usize;
        svc.push(Some(services[s].to_string()));
        host.push(Some(hosts[s].to_string()));
        let u = r.below(100);
        let mut acc = 0;
        for (k, &w) in next[state].iter().enumerate() { acc += w; if u < acc { state = k; break; } }
        level.push(Some(levels[state].to_string()));
        req += 1 + r.below(3) as i64;
        let ms = 3 + (r.unit() * r.unit() * 400.0) as i64;
        let st = if state == 3 { 500 } else if r.below(20) == 0 { 404 } else { 200 };
        let m = match r.below(6) {
            0 => format!("GET /api/users/{} {} {}ms", r.below(90_000), st, ms),
            1 => format!("user {} logged in from 10.0.{}.{}", r.below(5000), r.below(256), r.below(256)),
            2 => { job += 1; format!("job {} finished in {}s", job, r.below(120)) }
            3 => format!("cache hit ratio {}%", 80 + r.below(20)),
            4 => format!("POST /api/orders/{} {} {}ms", req, st, ms),
            _ => format!("request {} queued at depth {}", req, r.below(40)),
        };
        msg.push(Some(m));
        lat.push(Some(ms));
        status.push(Some(st));
        rid.push(Some(req));
    }
    DataBlock::new(vec![
        Column::str_col("ts", ts), Column::str_col("service", svc), Column::str_col("host", host), Column::str_col("level", level),
        Column::str_col("message", msg), Column::int64("latency_ms", lat), Column::int64("status", status), Column::int64("request_id", rid),
    ]).unwrap()
}

/// Integer sequences of the recurrence / polynomial families (wrapping where they overflow).
pub fn sequences(n: usize, seed: u64) -> DataBlock {
    let mut r = Rng::new(seed);
    let (mut fib, mut geo, mut quad, mut lin, mut noisy) = (vec![], vec![], vec![], vec![], vec![]);
    let (mut a, mut b) = (1i64, 1i64);
    let mut g = 7i64;
    for i in 0..n as i64 {
        fib.push(Some(a));
        let c = a.wrapping_add(b); a = b; b = c;
        geo.push(Some(g));
        g = g.wrapping_mul(3).wrapping_add(1);
        quad.push(Some(3 * i * i - 5 * i + 11));
        lin.push(Some(1000 + 17 * i));
        noisy.push(Some(1000 + 17 * i + if r.below(100) == 0 { r.below(1000) as i64 } else { 0 }));
    }
    DataBlock::new(vec![
        Column::int64("fibonacci", fib), Column::int64("affine_geometric", geo), Column::int64("quadratic", quad),
        Column::int64("arithmetic", lin), Column::int64("arithmetic_1pct_exceptions", noisy),
    ]).unwrap()
}

/// Control: no structure at all. Uniform 64-bit integers, doubles with uniform random bits (any bit
/// pattern, NaNs included), uniform doubles in [0, 1), 16-byte random hex text.
pub fn random(n: usize, seed: u64) -> DataBlock {
    let mut r = Rng::new(seed);
    let (mut i, mut fb, mut fu, mut s) = (vec![], vec![], vec![], vec![]);
    for _ in 0..n {
        i.push(Some(r.next() as i64));
        fb.push(Some(f64::from_bits(r.next())));
        fu.push(Some(r.unit()));
        s.push(Some(format!("{:016x}{:016x}", r.next(), r.next())));
    }
    DataBlock::new(vec![
        Column::int64("u64", i), Column::float64("f64_bits", fb), Column::float64("f64_unit", fu), Column::str_col("hex", s),
    ]).unwrap()
}
