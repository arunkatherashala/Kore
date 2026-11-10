//! Track E1, part 1: the genome, a rule expression of the being's DSL (`discover::Form::Gp`).
//!
//! A genome is an expression tree in prefix byte code (at most `MAX_NODES` nodes) plus an optional
//! modulus. It predicts the next term of a (possibly transformed) prefix from the previous terms:
//!
//! ```text
//! expr := t1 | t2 | t3            the term 1, 2 or 3 places back (t[-1], t[-2], t[-3])
//!       | n                       the index of the term (first term n = 0)
//!       | c<v>                    a small integer constant, -16..16 (part of the genome)
//!       | p                       a coefficient fitted per task from the task's own prefix
//!       | add(e, e) | sub(e, e) | mul(e, e)
//! rule := expr | expr mod m       m in 2..251 (part of the genome)
//! ```
//!
//! Coefficients `p` are fitted exactly as in B2: the expression must be affine in them (no product
//! of two sub-expressions that both contain a coefficient), every position of the prefix from the
//! largest lag on gives one equation, and the rule is APPLICABLE only if the system has a unique exact
//! solution that satisfies every equation with at least `MIN_SLACK` spare equations, the coefficients
//! are small (numerators up to 100, denominators up to 12) and the predicted term is an integer. With a
//! modulus the equations are solved modulo m (unique solution, pivots invertible modulo m), every
//! term the rule explains must lie in 0..m, and the modulus must be needed (the same expression without
//! it is not applicable to the prefix; otherwise the prefix shows no wrap and the rule defers). Every grammar form of B2 is a genome (`of_form`), so the
//! being's verified rules can be carried into the population as inherited genes.
//!
//! IMPORTANT: this expression language is written by the author, as the B2 grammar is; evolution
//! searches inside it. It is larger than the 17-form grammar plus transforms, not open-ended.

use super::super::discover::{solve, Form, Fr, MAX_DEN, MAX_LAG, MAX_NUM, MIN_SLACK};

pub const LEN: usize = 24;
pub const MAX_NODES: usize = LEN - 1;
pub const MAX_PARAMS: usize = 4;
pub const MAX_MOD: u8 = 251;
pub const C_MIN: i64 = -16;
pub const C_MAX: i64 = 16;

pub const END: u8 = 0;
pub const T1: u8 = 1;
pub const N: u8 = 4;
pub const P: u8 = 5;
pub const ADD: u8 = 6;
pub const SUB: u8 = 7;
pub const MUL: u8 = 8;
const C_BASE: u8 = 32;

pub fn t(k: usize) -> u8 {
    T1 + (k as u8 - 1)
}
pub fn c(v: i64) -> u8 {
    (C_BASE as i64 + v.clamp(C_MIN, C_MAX)) as u8
}
pub fn const_of(op: u8) -> Option<i64> {
    ((C_BASE as i64 + C_MIN) as u8..=(C_BASE as i64 + C_MAX) as u8).contains(&op).then(|| op as i64 - C_BASE as i64)
}
pub fn lag_of(op: u8) -> Option<usize> {
    (T1..T1 + MAX_LAG as u8).contains(&op).then(|| (op - T1 + 1) as usize)
}

/// Number of children of a node (None: not a node).
pub fn arity(op: u8) -> Option<usize> {
    match op {
        _ if lag_of(op).is_some() || const_of(op).is_some() => Some(0),
        N | P => Some(0),
        ADD | SUB | MUL => Some(2),
        _ => None,
    }
}

/// End (exclusive) of the subtree that starts at `i`.
pub fn subtree_end(ops: &[u8], i: usize) -> Option<usize> {
    let mut need = 1usize;
    let mut j = i;
    while need > 0 {
        let a = arity(*ops.get(j)?)?;
        need = need - 1 + a;
        j += 1;
    }
    Some(j)
}

/// (end, contains a coefficient) of the subtree at `i`; None if a product has coefficients on both sides.
fn affine_at(ops: &[u8], i: usize) -> Option<(usize, bool)> {
    let op = *ops.get(i)?;
    match arity(op)? {
        0 => Some((i + 1, op == P)),
        _ => {
            let (e1, p1) = affine_at(ops, i + 1)?;
            let (e2, p2) = affine_at(ops, e1)?;
            if op == MUL && p1 && p2 {
                return None;
            }
            Some((e2, p1 || p2))
        }
    }
}

/// A genome: byte 0 the modulus (0 = none), then the prefix code, padded with `END`.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Code(pub [u8; LEN]);

impl Code {
    /// A well-formed genome, or None (too long, not one tree, too many coefficients, not affine in them, bad modulus).
    pub fn new(modulus: u8, ops: &[u8]) -> Option<Code> {
        if ops.is_empty() || ops.len() > MAX_NODES || modulus == 1 || modulus > MAX_MOD {
            return None;
        }
        if subtree_end(ops, 0)? != ops.len() || ops.iter().filter(|&&o| o == P).count() > MAX_PARAMS {
            return None;
        }
        affine_at(ops, 0)?;
        let mut a = [END; LEN];
        a[0] = modulus;
        a[1..1 + ops.len()].copy_from_slice(ops);
        Some(Code(a))
    }
    pub fn ops(&self) -> &[u8] {
        let e = self.0[1..].iter().position(|&b| b == END).unwrap_or(MAX_NODES);
        &self.0[1..1 + e]
    }
    pub fn modulus(&self) -> u8 {
        self.0[0]
    }
    pub fn len(&self) -> usize {
        self.ops().len()
    }
    pub fn nparams(&self) -> usize {
        self.ops().iter().filter(|&&o| o == P).count()
    }
    pub fn maxlag(&self) -> usize {
        self.ops().iter().filter_map(|&o| lag_of(o)).max().unwrap_or(0)
    }

    fn token(op: u8) -> String {
        if let Some(k) = lag_of(op) {
            return format!("t{k}");
        }
        if let Some(v) = const_of(op) {
            return if v < 0 { format!("cm{}", -v) } else { format!("c{v}") };
        }
        match op {
            N => "n",
            P => "p",
            ADD => "add",
            SUB => "sub",
            MUL => "mul",
            _ => "?",
        }
        .into()
    }
    fn of_token(s: &str) -> Option<u8> {
        Some(match s {
            "n" => N,
            "p" => P,
            "add" => ADD,
            "sub" => SUB,
            "mul" => MUL,
            _ => {
                if let Some(k) = s.strip_prefix('t') {
                    let k: usize = k.parse().ok()?;
                    if !(1..=MAX_LAG).contains(&k) {
                        return None;
                    }
                    t(k)
                } else if let Some(v) = s.strip_prefix("cm") {
                    let v: i64 = v.parse().ok()?;
                    if !(1..=-C_MIN).contains(&v) {
                        return None;
                    }
                    c(-v)
                } else if let Some(v) = s.strip_prefix('c') {
                    let v: i64 = v.parse().ok()?;
                    if !(0..=C_MAX).contains(&v) {
                        return None;
                    }
                    c(v)
                } else {
                    return None;
                }
            }
        })
    }

    /// Stable code for tags and ids: `gp.mul_t1_t2`, `gp.m101.add_mul_p_t1_p`.
    pub fn code_str(&self) -> String {
        let toks: Vec<String> = self.ops().iter().map(|&o| Code::token(o)).collect();
        match self.modulus() {
            0 => format!("gp.{}", toks.join("_")),
            m => format!("gp.m{m}.{}", toks.join("_")),
        }
    }
    pub fn parse(s: &str) -> Option<Code> {
        let rest = s.strip_prefix("gp.")?;
        let (m, body) = match rest.split_once('.') {
            Some((m, b)) => (m.strip_prefix('m')?.parse::<u8>().ok().filter(|m| *m >= 2)?, b),
            None => (0, rest),
        };
        let ops: Option<Vec<u8>> = body.split('_').map(Code::of_token).collect();
        let g = Code::new(m, &ops?)?;
        (g.code_str() == s).then_some(g)
    }

    /// The rule as text: `next = a1*t[-1] + a2   (a1, a2 fitted per task)`.
    pub fn dsl(&self) -> String {
        fn go(ops: &[u8], i: usize, k: &mut usize) -> (usize, String) {
            let op = ops[i];
            if let Some(l) = lag_of(op) {
                return (i + 1, format!("t[-{l}]"));
            }
            if let Some(v) = const_of(op) {
                return (i + 1, v.to_string());
            }
            match op {
                N => (i + 1, "n".into()),
                P => {
                    *k += 1;
                    (i + 1, format!("a{k}"))
                }
                _ => {
                    let (e1, a) = go(ops, i + 1, k);
                    let (e2, b) = go(ops, e1, k);
                    let s = match op {
                        ADD => format!("({a} + {b})"),
                        SUB => format!("({a} - {b})"),
                        _ => format!("{a}*{b}"),
                    };
                    (e2, s)
                }
            }
        }
        let mut k = 0;
        let (_, mut e) = go(self.ops(), 0, &mut k);
        if matches!(self.ops()[0], ADD | SUB) {
            e = e[1..e.len() - 1].to_string();
        }
        let m = match self.modulus() {
            0 => String::new(),
            m => format!(", taken modulo {m}"),
        };
        let fit = if k > 0 { format!("; {} fitted per task", (1..=k).map(|j| format!("a{j}")).collect::<Vec<_>>().join(", ")) } else { String::new() };
        format!("next = {e}{m}   (evolved expression; n = index of the term, first term n = 0{fit})")
    }
}

/// The value of an expression at one position: c[0] + sum_j c[j + 1] * coefficient_j.
type Aff = [i128; MAX_PARAMS + 1];
const LIM: i128 = 1 << 62;

fn eval(ops: &[u8], i: usize, t: &[i64], pos: usize, pk: &mut usize) -> Option<(usize, Aff)> {
    let op = ops[i];
    let mut a: Aff = [0; MAX_PARAMS + 1];
    if let Some(l) = lag_of(op) {
        a[0] = t[pos.checked_sub(l)?] as i128;
        return Some((i + 1, a));
    }
    if let Some(v) = const_of(op) {
        a[0] = v as i128;
        return Some((i + 1, a));
    }
    match op {
        N => {
            a[0] = pos as i128;
            Some((i + 1, a))
        }
        P => {
            a[1 + *pk] = 1;
            *pk += 1;
            Some((i + 1, a))
        }
        _ => {
            let (e1, x) = eval(ops, i + 1, t, pos, pk)?;
            let (e2, y) = eval(ops, e1, t, pos, pk)?;
            let xc = x[1..].iter().all(|v| *v == 0);
            let yc = y[1..].iter().all(|v| *v == 0);
            for j in 0..=MAX_PARAMS {
                a[j] = match op {
                    ADD => x[j].checked_add(y[j])?,
                    SUB => x[j].checked_sub(y[j])?,
                    _ if xc => y[j].checked_mul(x[0])?,
                    _ if yc => x[j].checked_mul(y[0])?,
                    _ => return None,
                };
                if a[j].abs() >= LIM {
                    return None;
                }
            }
            Some((e2, a))
        }
    }
}

/// The equations of the prefix (positions maxlag..len) and the row of the next term. None: overflow.
fn system(g: &Code, t: &[i64]) -> Option<(Vec<Aff>, Aff, usize)> {
    let ml = g.maxlag();
    if t.len() <= ml {
        return None;
    }
    let ops = g.ops();
    let mut rows = vec![];
    for pos in ml..=t.len() {
        let mut pk = 0;
        rows.push(eval(ops, 0, t, pos, &mut pk)?.1);
    }
    let next = rows.pop()?;
    Some((rows, next, ml))
}

fn inv_mod(a: i128, m: i128) -> Option<i128> {
    let (mut r0, mut r1, mut s0, mut s1) = (a.rem_euclid(m), m, 1i128, 0i128);
    while r1 != 0 {
        let q = r0 / r1;
        (r0, r1) = (r1, r0 - q * r1);
        (s0, s1) = (s1, s0 - q * s1);
    }
    (r0 == 1).then(|| s0.rem_euclid(m))
}

/// Unique solution of `rows * x = rhs (mod m)` with invertible pivots; None if inconsistent or not unique.
fn solve_mod(rows: &[Vec<i128>], rhs: &[i128], m: i128) -> Option<Vec<i128>> {
    let n = rows.first()?.len();
    let mut a: Vec<Vec<i128>> = rows.iter().zip(rhs).map(|(r, y)| r.iter().chain(std::iter::once(y)).map(|v| v.rem_euclid(m)).collect()).collect();
    for col in 0..n {
        let p = (col..a.len()).find(|&i| inv_mod(a[i][col], m).is_some())?;
        a.swap(col, p);
        let iv = inv_mod(a[col][col], m)?;
        for j in 0..=n {
            a[col][j] = a[col][j] * iv % m;
        }
        for i in 0..a.len() {
            if i != col && a[i][col] != 0 {
                let f = a[i][col];
                for j in 0..=n {
                    a[i][j] = (a[i][j] - f * a[col][j]).rem_euclid(m);
                }
            }
        }
    }
    if a[n..].iter().any(|r| r.iter().any(|v| *v != 0)) {
        return None;
    }
    Some((0..n).map(|i| a[i][n]).collect())
}

/// Fit the genome to the prefix `t` and predict the next term. None = not applicable (module doc).
pub fn predict(g: &Code, t: &[i64]) -> Option<i64> {
    let (rows, next, ml) = system(g, t)?;
    let np = g.nparams();
    if rows.len() < np + MIN_SLACK {
        return None;
    }
    let m = g.modulus() as i128;
    let obs = &t[ml..];
    if m > 0 {
        if obs.iter().any(|v| *v < 0 || *v as i128 >= m) {
            return None;
        }
        // the modulus must be needed: where the same expression without it already describes the
        // prefix, the prefix gives no evidence of wrapping and the rule is not applied
        if Code::new(0, g.ops()).map_or(false, |plain| predict(&plain, t).is_some()) {
            return None;
        }
        if np == 0 {
            if rows.iter().zip(obs).any(|(r, y)| r[0].rem_euclid(m) != *y as i128) {
                return None;
            }
            return i64::try_from(next[0].rem_euclid(m)).ok();
        }
        let a: Vec<Vec<i128>> = rows.iter().map(|r| r[1..=np].to_vec()).collect();
        let rhs: Vec<i128> = rows.iter().zip(obs).map(|(r, y)| *y as i128 - r[0]).collect();
        let x = solve_mod(&a, &rhs, m)?;
        let mut v = next[0].rem_euclid(m);
        for j in 0..np {
            v = (v + next[1 + j].rem_euclid(m) * x[j]) % m;
        }
        return i64::try_from(v).ok();
    }
    if np == 0 {
        if rows.iter().zip(obs).any(|(r, y)| r[0] != *y as i128) {
            return None;
        }
        return i64::try_from(next[0]).ok();
    }
    let a: Vec<Vec<i64>> = rows.iter().map(|r| r[1..=np].iter().map(|v| i64::try_from(*v).ok()).collect::<Option<Vec<i64>>>()).collect::<Option<_>>()?;
    let rhs: Vec<i64> = rows.iter().zip(obs).map(|(r, y)| i64::try_from(*y as i128 - r[0]).ok()).collect::<Option<_>>()?;
    let x = solve(&a, &rhs)?;
    let mut acc = Fr::int(next[0]);
    for (j, cj) in x.iter().enumerate() {
        if cj.n.abs() > MAX_NUM || cj.d > MAX_DEN {
            return None;
        }
        acc = acc.add(cj.mul(Fr::int(next[1 + j]))?)?;
    }
    if acc.d != 1 {
        return None;
    }
    i64::try_from(acc.n).ok()
}

/// Partial credit for the search only (never for acceptance): the share of the prefix equations that
/// hold when the coefficients are fitted on the first equations alone. 0 when it cannot be computed.
pub fn partial(g: &Code, t: &[i64]) -> f64 {
    let Some((rows, _, ml)) = system(g, t) else { return 0.0 };
    let np = g.nparams();
    if rows.len() <= np {
        return 0.0;
    }
    let obs = &t[ml..];
    let m = g.modulus() as i128;
    let hits = if m > 0 {
        let x = if np == 0 {
            vec![]
        } else {
            let a: Vec<Vec<i128>> = rows[..np].iter().map(|r| r[1..=np].to_vec()).collect();
            let rhs: Vec<i128> = rows[..np].iter().zip(obs).map(|(r, y)| *y as i128 - r[0]).collect();
            match solve_mod(&a, &rhs, m) {
                Some(x) => x,
                None => return 0.0,
            }
        };
        rows.iter().zip(obs).filter(|(r, y)| {
            let mut v = r[0].rem_euclid(m);
            for j in 0..np {
                v = (v + r[1 + j].rem_euclid(m) * x[j]) % m;
            }
            v == **y as i128
        }).count()
    } else {
        let x: Vec<Fr> = if np == 0 {
            vec![]
        } else {
            let a: Option<Vec<Vec<i64>>> = rows[..np].iter().map(|r| r[1..=np].iter().map(|v| i64::try_from(*v).ok()).collect()).collect();
            let rhs: Option<Vec<i64>> = rows[..np].iter().zip(obs).map(|(r, y)| i64::try_from(*y as i128 - r[0]).ok()).collect();
            match (a, rhs) {
                (Some(a), Some(rhs)) => match solve(&a, &rhs) {
                    Some(x) => x,
                    None => return 0.0,
                },
                _ => return 0.0,
            }
        };
        rows.iter().zip(obs).filter(|(r, y)| {
            let mut acc = Fr::int(r[0]);
            for (j, cj) in x.iter().enumerate() {
                match cj.mul(Fr::int(r[1 + j])).and_then(|v| acc.add(v)) {
                    Some(v) => acc = v,
                    None => return false,
                }
            }
            acc.d == 1 && acc.n == **y as i128
        }).count()
    };
    hits as f64 / rows.len() as f64
}

/// A grammar form of B2 as a genome (the inherited genes); an evolved form as itself.
pub fn of_form(f: &Form) -> Option<Code> {
    let sum = |terms: Vec<Vec<u8>>| -> Option<Code> {
        let mut ops = vec![ADD; terms.len().saturating_sub(1)];
        for x in terms {
            ops.extend(x);
        }
        Code::new(0, &ops)
    };
    match f {
        Form::Lin { mask, constant } => {
            let mut terms: Vec<Vec<u8>> = (1..=MAX_LAG).filter(|l| mask & (1 << (l - 1)) != 0).map(|l| vec![MUL, P, t(l)]).collect();
            if *constant {
                terms.push(vec![P]);
            }
            sum(terms)
        }
        Form::Poly(d) => {
            let mut terms = vec![vec![P]];
            for j in 1..=*d as usize {
                let mut x = vec![MUL, P];
                x.extend(std::iter::repeat(MUL).take(j - 1));
                x.extend(std::iter::repeat(N).take(j));
                terms.push(x);
            }
            sum(terms)
        }
        Form::Gp(g) => Some(*g),
    }
}
