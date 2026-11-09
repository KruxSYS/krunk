//! Fractional indices for bookmark order (PROTOCOL §9.5): base-62 strings that sort between
//! their neighbours, so a move changes one item's `sort` only. Siblings are ordered by
//! (`sort`, `guid`). The midpoint algorithm is the well-known one of rocicorp's
//! `fractional-indexing` (without the integer part): keys are non-empty and never end in `0`,
//! so there is always room between two of them.

const DIGITS: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

fn digit(c: u8) -> Option<usize> {
    DIGITS.iter().position(|d| *d == c)
}

/// A valid key: non-empty, base-62 digits only, not ending in `0`, at most 128 characters.
pub fn valid(key: &str) -> bool {
    !key.is_empty() && key.len() <= 128 && key.bytes().all(|b| digit(b).is_some()) && !key.ends_with('0')
}

/// A key strictly between `a` and `b` (`None`: the start or the end). `a` must sort before `b`;
/// invalid or misordered neighbours are treated as absent.
pub fn between(a: Option<&str>, b: Option<&str>) -> String {
    let a = a.filter(|x| valid(x)).unwrap_or("");
    let b = b.filter(|x| valid(x) && *x > a);
    midpoint(a.as_bytes(), b.map(str::as_bytes))
}

fn midpoint(a: &[u8], b: Option<&[u8]>) -> String {
    if let Some(b) = b {
        let mut n = 0;
        while n < b.len() && a.get(n).copied().unwrap_or(b'0') == b[n] {
            n += 1;
        }
        if n > 0 {
            let rest_a = if n <= a.len() { &a[n..] } else { &[][..] };
            return String::from_utf8_lossy(&b[..n]).into_owned() + &midpoint(rest_a, Some(&b[n..]));
        }
    }
    let da = a.first().and_then(|c| digit(*c)).unwrap_or(0);
    let db = match b {
        Some(b) => b.first().and_then(|c| digit(*c)).unwrap_or(DIGITS.len()),
        None => DIGITS.len(),
    };
    if db - da > 1 {
        return (DIGITS[(da + db).div_ceil(2)] as char).to_string();
    }
    match b {
        Some(b) if b.len() > 1 => String::from_utf8_lossy(&b[..1]).into_owned(),
        _ => {
            let rest = if a.is_empty() { &[][..] } else { &a[1..] };
            (DIGITS[da] as char).to_string() + &midpoint(rest, None)
        }
    }
}

/// `n` evenly spread keys in order (the first sync of a folder).
pub fn spread(n: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(n);
    for _ in 0..n {
        let k = between(out.last().map(String::as_str), None);
        out.push(k);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_sort_between_their_neighbours() {
        let first = between(None, None);
        assert!(valid(&first));
        let mut keys = vec![first];
        // Inserting at the front, the back and in every gap, many times, keeps the order.
        for round in 0..200 {
            let i = (round * 7) % (keys.len() + 1);
            let a = if i == 0 { None } else { Some(keys[i - 1].as_str()) };
            let b = keys.get(i).map(String::as_str);
            let k = between(a, b);
            assert!(valid(&k), "{k}");
            if let Some(a) = a {
                assert!(a < k.as_str(), "{a} < {k}");
            }
            if let Some(b) = b {
                assert!(k.as_str() < b, "{k} < {b}");
            }
            keys.insert(i, k);
        }
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
        // Always inserting after the same key grows slowly.
        let mut k = between(None, None);
        let next = between(Some("V"), Some("W"));
        assert!("V" < next.as_str() && next.as_str() < "W");
        for _ in 0..100 {
            k = between(None, Some(&k));
        }
        assert!(k.len() < 40, "{}", k.len());
        let s = spread(5);
        assert!(s.windows(2).all(|w| w[0] < w[1]));
        // Garbage neighbours are ignored.
        assert!(valid(&between(Some("bad!"), Some("x0"))));
        assert!(valid(&between(Some("z"), Some("a"))));
    }
}
