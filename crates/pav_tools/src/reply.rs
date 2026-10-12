//! Reply hygiene shared by every tool. Engine values are f32; widened to f64 they print as
//! noise (`1.1699999570846558`). One pass in `tools::call` covers the CLI, REPL, MCP and both
//! bridges.

use serde_json::{Number, Value};

/// Every number that holds an exact f32 becomes that f32's shortest decimal, and integral
/// numbers become integers. Lossless for engine values: each one reads back as the same f32.
/// Genuine f64 values (timings, rates) keep their digits.
pub fn tidy(value: &mut Value) {
    match value {
        Value::Number(number) => {
            if let Some(tidied) = tidy_number(number) {
                *number = tidied;
            }
        }
        Value::Array(items) => items.iter_mut().for_each(tidy),
        Value::Object(map) => map.values_mut().for_each(tidy),
        _ => {}
    }
}

fn tidy_number(number: &Number) -> Option<Number> {
    if !number.is_f64() {
        return None;
    }
    let mut x = number.as_f64()?;
    let single = x as f32;
    if f64::from(single) == x {
        // Display prints the shortest decimal that reads back as this f32, never an exponent.
        x = single.to_string().parse().ok()?;
    }
    if x.fract() == 0.0 && x.abs() < 9_007_199_254_740_992.0 {
        return Some(Number::from(x as i64));
    }
    Number::from_f64(x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn f32_noise_prints_short_and_integers_lose_their_point() {
        let mut value = json!({
            "pos": [f64::from(1.17_f32), f64::from(-0.36_f32), 0.0],
            "pitch": -12.0, "zero": -0.0, "count": 7,
            "nested": {"half": [f64::from(0.025_f32)], "big": f64::from(1.0e20_f32)},
            "apply_ms": 0.123456789, "text": "1.1699999570846558"
        });
        tidy(&mut value);
        assert_eq!(
            value.to_string(),
            r#"{"apply_ms":0.123456789,"count":7,"nested":{"big":1e+20,"half":[0.025]},"pitch":-12,"pos":[1.17,-0.36,0],"text":"1.1699999570846558","zero":0}"#
        );
        // Lossless for f32 readers.
        for x in [1.17_f32, -0.36, 0.025, 1.0e20, 3.4028235e38, 1.0e-30, 0.1 + 0.2] {
            let mut value = json!(f64::from(x));
            tidy(&mut value);
            assert_eq!(value.as_f64().unwrap() as f32, x);
        }
    }
}
