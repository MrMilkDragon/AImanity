use std::fmt;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Version {
    pub base: [u32; 3],
    pub fix: u32,
}

impl Version {
    pub fn parse(label: &str) -> Result<Self, &'static str> {
        let (base, fix) = label
            .split_once(".fix")
            .or_else(|| label.split_once("+fix."))
            .map_or((label, None), |(base, fix)| (base, Some(fix)));
        let number = |part: &str| {
            if part.is_empty()
                || (part.len() > 1 && part.starts_with('0'))
                || !part.bytes().all(|b| b.is_ascii_digit())
            {
                return Err("Invalid release version");
            }
            part.parse::<u32>().map_err(|_| "Invalid release version")
        };
        let mut parts = base.split('.');
        let base = [
            number(parts.next().unwrap_or(""))?,
            number(parts.next().unwrap_or(""))?,
            number(parts.next().unwrap_or(""))?,
        ];
        if parts.next().is_some() {
            return Err("Invalid release version");
        }
        let fix = match fix {
            Some(s) => number(s).and_then(|n| {
                if n > 0 {
                    Ok(n)
                } else {
                    Err("Invalid patch number")
                }
            })?,
            None => 0,
        };
        Ok(Self { base, fix })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [major, minor, patch] = self.base;
        write!(f, "{major}.{minor}.{patch}")?;
        if self.fix > 0 {
            write!(f, ".fix{}", self.fix)?;
        }
        Ok(())
    }
}
