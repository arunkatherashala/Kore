//! Physics, chemistry, and space-science solvers for the World Solver.

#[derive(Debug, Clone)]
pub struct ScienceAnswer {
    pub method: String,
    pub answer: String,
    pub confidence: f64,
}

const G: f64 = 6.674_30e-11;
const C: f64 = 299_792_458.0;
const R_GAS: f64 = 8.314_462_618;
const G_EARTH: f64 = 9.806_65;
const M_EARTH: f64 = 5.972_2e24;
const R_EARTH: f64 = 6.371e6;
const AU_KM: f64 = 149_597_870.7;
const SUN_EARTH_KM: f64 = 149.6e6;

pub fn try_science(problem: &str, steps: &mut Vec<String>) -> Option<ScienceAnswer> {
    let lower = problem.to_lowercase();
    // Try every matching domain (physics, chemistry, space) — first hit wins.
    let physics = is_physics(&lower);
    let chemistry = is_chemistry(&lower);
    let space = is_space(&lower);
    if !physics && !chemistry && !space {
        return None;
    }
    if physics {
        if let Some(r) = try_physics(problem, &lower, steps) {
            return Some(r);
        }
    }
    if chemistry {
        if let Some(r) = try_chemistry(problem, &lower, steps) {
            return Some(r);
        }
    }
    if space {
        if let Some(r) = try_space(problem, &lower, steps) {
            return Some(r);
        }
    }
    None
}

fn is_physics(lower: &str) -> bool {
    [
        "force", "newton", "velocity", "acceleration", "momentum", "kinetic", "potential energy",
        "gravity", "work", "joule", "watt", "power", "pressure", "ohm", "voltage", "current",
        "physics", "friction", "inertia", "density", "wavelength", "frequency",
    ]
    .iter()
    .any(|k| lower.contains(k))
}

fn is_chemistry(lower: &str) -> bool {
    [
        "molar", "mol ", "mole", "chemistry", "compound", "molecule", "ph ", "ph=", "acid",
        "ideal gas", "gas law", "pv=nrt", "concentration", "dilution", "h2o", "co2", "nacl",
        "stoichiometry", "reactant", "product", "balance", "equation",
    ]
    .iter()
    .any(|k| lower.contains(k))
}

fn is_space(lower: &str) -> bool {
    [
        "orbit", "space", "planet", "moon", "sun", "earth", "light year", "au ",
        "astronomical", "escape velocity", "rocket", "satellite", "cosmos", "galaxy",
        "lunar", "moon gravity", "black hole", "nebula",
    ]
    .iter()
    .any(|k| lower.contains(k))
}

fn try_physics(problem: &str, lower: &str, steps: &mut Vec<String>) -> Option<ScienceAnswer> {
    let n = parse_numbers(problem);
    let labeled = parse_labeled(problem);

    if lower.contains("force") || lower.contains("newton") {
        if let (Some(m), Some(a)) = (
            labeled.get("mass").or(labeled.get("m")),
            labeled.get("acceleration").or(labeled.get("a")),
        ) {
            let f = m * a;
            steps.push(format!("Newton II: F = m × a = {m} × {a}"));
            return Some(ans("physics_force", format!("F = {f:.6} N"), 0.91));
        }
        if n.len() >= 2 {
            let f = n[0] * n[1];
            steps.push(format!("F = m × a = {} × {}", n[0], n[1]));
            return Some(ans("physics_force", format!("F = {f:.6} N"), 0.85));
        }
    }

    if lower.contains("kinetic") {
        let m = labeled.get("mass").or(labeled.get("m")).copied().or_else(|| n.first().copied());
        let v = labeled
            .get("velocity")
            .or(labeled.get("v").or(labeled.get("speed")))
            .copied()
            .or_else(|| n.get(1).copied());
        if let (Some(m), Some(v)) = (m, v) {
            let ek = 0.5 * m * v * v;
            steps.push(format!("E_k = ½mv² = 0.5 × {m} × {v}²"));
            return Some(ans("physics_kinetic_energy", format!("E_k = {ek:.6} J"), 0.91));
        }
    }

    if lower.contains("potential") || (lower.contains("gravity") && lower.contains("height")) {
        let m = labeled.get("mass").or(labeled.get("m")).copied().or_else(|| n.first().copied());
        let h = labeled.get("height").or(labeled.get("h")).copied().or_else(|| n.get(1).copied());
        if let (Some(m), Some(h)) = (m, h) {
            let ep = m * G_EARTH * h;
            steps.push(format!("E_p = mgh = {m} × {G_EARTH} × {h}"));
            return Some(ans("physics_potential_energy", format!("E_p = {ep:.6} J"), 0.9));
        }
    }

    if lower.contains("momentum") {
        if let (Some(m), Some(v)) = (
            labeled.get("mass").copied().or_else(|| n.first().copied()),
            labeled
                .get("velocity")
                .or(labeled.get("v"))
                .copied()
                .or_else(|| n.get(1).copied()),
        ) {
            let p = m * v;
            steps.push(format!("p = mv = {m} × {v}"));
            return Some(ans("physics_momentum", format!("p = {p:.6} kg·m/s"), 0.9));
        }
    }

    if lower.contains("ohm") || (lower.contains("voltage") && lower.contains("current")) {
        let i = labeled.get("current").or(labeled.get("i")).copied();
        let r = labeled.get("resistance").or(labeled.get("r")).copied();
        let v = labeled.get("voltage").or(labeled.get("v")).copied();
        if let (Some(i), Some(r)) = (i, r) {
            steps.push(format!("Ohm's law: V = I×R = {i}×{r}"));
            return Some(ans("physics_ohms", format!("V = {:.6} V", i * r), 0.92));
        }
        if let (Some(v), Some(r)) = (v, r) {
            if r != 0.0 {
                steps.push(format!("I = V/R = {v}/{r}"));
                return Some(ans("physics_ohms", format!("I = {:.6} A", v / r), 0.92));
            }
        }
    }

    if lower.contains("work") && !lower.contains("network") && n.len() >= 2 {
        let w = n[0] * n[1];
        steps.push(format!("W = F × d = {} × {}", n[0], n[1]));
        return Some(ans("physics_work", format!("W = {w:.6} J"), 0.84));
    }

    if lower.contains("density") {
        let m = labeled.get("mass").copied().or_else(|| n.first().copied());
        let v = labeled.get("volume").copied().or_else(|| n.get(1).copied());
        if let (Some(m), Some(v)) = (m, v) {
            if v != 0.0 {
                let rho = m / v;
                steps.push(format!("ρ = m/V = {m}/{v}"));
                return Some(ans("physics_density", format!("ρ = {rho:.6} kg/m³"), 0.89));
            }
        }
    }

    if lower.contains("power") && (lower.contains("voltage") || lower.contains("current") || labeled.contains_key("voltage")) {
        let v = labeled.get("voltage").copied();
        let i = labeled.get("current").copied();
        if let (Some(v), Some(i)) = (v, i) {
            steps.push(format!("P = V×I = {v}×{i}"));
            return Some(ans("physics_power", format!("P = {:.6} W", v * i), 0.9));
        }
    }

    if (lower.contains("wavelength") || lower.contains("frequency")) && !n.is_empty() {
        if lower.contains("wavelength") {
            if let Some(lambda) = n.first() {
                if *lambda > 0.0 {
                    let f = C / lambda;
                    steps.push(format!("f = c/λ = {C}/{lambda}"));
                    return Some(ans("physics_frequency", format!("f ≈ {f:.6e} Hz"), 0.88));
                }
            }
        } else if let Some(f) = n.first() {
            if *f > 0.0 {
                let lambda = C / f;
                steps.push(format!("λ = c/f = {C}/{f}"));
                return Some(ans("physics_wavelength", format!("λ ≈ {lambda:.6e} m"), 0.88));
            }
        }
    }

    None
}

fn try_chemistry(problem: &str, lower: &str, steps: &mut Vec<String>) -> Option<ScienceAnswer> {
    if lower.contains("molar mass") || lower.contains("molecular mass") {
        if let Some(mm) = molar_mass_lookup(lower) {
            steps.push("Formula → standard molar mass (g/mol)".into());
            return Some(ans(
                "chemistry_molar_mass",
                format!("Molar mass ≈ {mm:.4} g/mol"),
                0.93,
            ));
        }
    }

    if (lower.contains("mole") || lower.contains("mol ")) && !lower.contains("molar mass") {
        if let Some(mm) = molar_mass_lookup(lower) {
            let n = parse_numbers(problem);
            if let Some(mass) = n.first() {
                let moles = mass / mm;
                steps.push(format!("n = mass/M = {mass}/{mm:.4} g/mol"));
                return Some(ans("chemistry_moles", format!("n = {moles:.6} mol"), 0.9));
            }
        }
    }

    if lower.contains("ideal gas") || lower.contains("pv=nrt") || lower.contains("gas law") {
        let n = parse_numbers(problem);
        if n.len() >= 3 && n[1] != 0.0 {
            steps.push("PV = nRT → P = nRT/V (SI units)".into());
            let p = n[0] * R_GAS * n[2] / n[1];
            return Some(ans(
                "chemistry_ideal_gas",
                format!("P ≈ {p:.2} Pa"),
                0.85,
            ));
        }
    }

    if lower.contains("ph") {
        let n = parse_numbers(problem);
        if let Some(h) = n.first() {
            if *h > 0.0 {
                let ph = -h.log10();
                steps.push(format!("pH = -log10[H+] = -log10({h})"));
                return Some(ans("chemistry_ph", format!("pH ≈ {ph:.4}"), 0.9));
            }
        }
    }

    if lower.contains("dilution") && parse_numbers(problem).len() >= 4 {
        let n = parse_numbers(problem);
        let c2 = n[0] * n[1] / n[3];
        steps.push(format!("C1V1 = C2V2 → C2 = {}×{}/{}", n[0], n[1], n[3]));
        return Some(ans("chemistry_dilution", format!("C2 ≈ {c2:.6}"), 0.87));
    }

    if lower.contains("stoichiometry") || (lower.contains("balance") && lower.contains("equation")) {
        steps.push("Stoichiometry: use mole ratios from balanced equation.".into());
        if let Some(mm) = molar_mass_lookup(lower) {
            let n = parse_numbers(problem);
            if let Some(mass) = n.first() {
                let moles = mass / mm;
                return Some(ans(
                    "chemistry_stoichiometry",
                    format!("Starting amount ≈ {moles:.6} mol (from {mass} g, M≈{mm:.3} g/mol); scale by coefficients."),
                    0.78,
                ));
            }
        }
        return Some(ans(
            "chemistry_stoichiometry",
            "Name reactants/products and masses or moles; KORE will apply mole ratios.".into(),
            0.5,
        ));
    }

    None
}

fn try_space(problem: &str, lower: &str, steps: &mut Vec<String>) -> Option<ScienceAnswer> {
    let n = parse_numbers(problem);

    if lower.contains("light") && (lower.contains("sun") || lower.contains("earth") || lower.contains("travel")) {
        let km = if lower.contains("sun") && lower.contains("earth") {
            SUN_EARTH_KM
        } else {
            n.first().copied().unwrap_or(SUN_EARTH_KM)
        };
        let secs = (km * 1000.0) / C;
        steps.push(format!("t = distance/c, distance = {km} km"));
        return Some(ans(
            "space_light_time",
            format!("Light travel time ≈ {secs:.2} s ({:.2} min)", secs / 60.0),
            0.92,
        ));
    }

    if lower.contains("au") && (lower.contains("km") || lower.contains("kilometer")) {
        let au = n.first().copied().unwrap_or(1.0);
        steps.push(format!("1 AU = {AU_KM} km"));
        return Some(ans(
            "space_au",
            format!("{au} AU = {:.3} km", au * AU_KM),
            0.94,
        ));
    }

    if lower.contains("escape velocity") {
        let r = if lower.contains("earth") {
            R_EARTH
        } else {
            n.first().copied().unwrap_or(R_EARTH)
        };
        let m = if lower.contains("earth") {
            M_EARTH
        } else {
            n.get(1).copied().unwrap_or(M_EARTH)
        };
        let v = (2.0 * G * m / r).sqrt();
        steps.push("v_esc = √(2GM/r)".into());
        return Some(ans(
            "space_escape_velocity",
            format!("v_esc ≈ {v:.2} m/s ({:.2} km/s)", v / 1000.0),
            0.9,
        ));
    }

    if lower.contains("orbit") || lower.contains("orbital velocity") {
        let alt_km = n.first().copied().unwrap_or(400.0);
        let r = R_EARTH + alt_km * 1000.0;
        let v = (G * M_EARTH / r).sqrt();
        steps.push(format!("v = √(GM/r), altitude {alt_km} km above Earth"));
        return Some(ans(
            "space_orbital_velocity",
            format!("Orbital speed ≈ {v:.2} m/s ({:.2} km/s)", v / 1000.0),
            0.88,
        ));
    }

    if lower.contains("period") && lower.contains("orbit") {
        let alt_km = n.first().copied().unwrap_or(400.0);
        let r = R_EARTH + alt_km * 1000.0;
        let v = (G * M_EARTH / r).sqrt();
        let period = 2.0 * std::f64::consts::PI * r / v;
        steps.push(format!("T = 2πr/v at {alt_km} km altitude"));
        return Some(ans(
            "space_orbital_period",
            format!("Orbital period ≈ {period:.1} s ({:.2} min)", period / 60.0),
            0.86,
        ));
    }

    if lower.contains("mars") && (lower.contains("gravity") || lower.contains("g ")) {
        const M_MARS: f64 = 6.4171e23;
        const R_MARS: f64 = 3.3895e6;
        let g = G * M_MARS / (R_MARS * R_MARS);
        steps.push("g_Mars = GM/r²".into());
        return Some(ans(
            "space_mars_gravity",
            format!("Surface gravity on Mars ≈ {g:.3} m/s²"),
            0.91,
        ));
    }

    if lower.contains("moon") && lower.contains("gravity") {
        const M_MOON: f64 = 7.342e22;
        const R_MOON: f64 = 1.737e6;
        let g = G * M_MOON / (R_MOON * R_MOON);
        steps.push("g_Moon = GM/r²".into());
        return Some(ans(
            "space_moon_gravity",
            format!("Surface gravity on the Moon ≈ {g:.3} m/s²"),
            0.91,
        ));
    }

    None
}

fn ans(method: &str, answer: String, confidence: f64) -> ScienceAnswer {
    ScienceAnswer {
        method: method.to_string(),
        answer,
        confidence,
    }
}

fn molar_mass_lookup(lower: &str) -> Option<f64> {
    const FORMULAS: &[(&str, f64)] = &[
        ("h2o", 18.015),
        ("water", 18.015),
        ("co2", 44.009),
        ("o2", 31.998),
        ("n2", 28.014),
        ("ch4", 16.043),
        ("methane", 16.043),
        ("nacl", 58.44),
        ("salt", 58.44),
        ("nh3", 17.031),
        ("ammonia", 17.031),
        ("h2so4", 98.079),
        ("c6h12o6", 180.156),
        ("glucose", 180.156),
        ("naoh", 39.997),
        ("hcl", 36.458),
        ("c2h5oh", 46.068),
        ("ethanol", 46.068),
    ];
    for (name, mm) in FORMULAS {
        if lower.contains(name) {
            return Some(*mm);
        }
    }
    None
}

fn parse_numbers(s: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            cur.push(ch);
        } else if !cur.is_empty() {
            if let Ok(v) = cur.parse() {
                out.push(v);
            }
            cur.clear();
        }
    }
    if !cur.is_empty() {
        if let Ok(v) = cur.parse() {
            out.push(v);
        }
    }
    out
}

fn parse_labeled(s: &str) -> std::collections::HashMap<String, f64> {
    let mut map = std::collections::HashMap::new();
    let lower = s.to_lowercase();
    const LABEL_ALIASES: &[(&str, &[&str])] = &[
        ("mass", &["mass", "m="]),
        ("velocity", &["velocity", "speed", "v="]),
        ("acceleration", &["acceleration", "a="]),
        ("time", &["time", "t="]),
        ("height", &["height", "h="]),
        ("current", &["current", "i="]),
        ("resistance", &["resistance", "r="]),
        ("voltage", &["voltage", "v="]),
        ("volume", &["volume", "vol", "v="]),
        ("temperature", &["temperature", "temp", "t="]),
    ];
    for (key, aliases) in LABEL_ALIASES {
        for alias in *aliases {
            if let Some(idx) = lower.find(alias) {
                let rest = &s[idx + alias.len()..];
                let rest = rest.trim_start_matches('=').trim_start();
                if let Some(num) = parse_numbers(rest).first() {
                    map.insert(key.to_string(), *num);
                    break;
                }
            }
        }
    }
    map
}
