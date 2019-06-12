//! configuration for network inflation

#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct Inflation {
    /// Initial inflation percentage, from time=0
    pub initial: f64,

    /// Terminal inflation percentage, to time=INF
    pub terminal: f64,

    /// Rate per year, at which inflation is lowered until reaching terminal
    ///  i.e. inflation(year) == MAX(terminal, initial*((1-taper)^year))
    pub taper: f64,

    /// Percentage of total inflation allocated to the foundation
    pub foundation: f64,
    /// Duration of foundationt pool inflation, in years
    pub foundation_term: f64,

    /// Percentage of total inflation allocated to grant pools
    pub grant: f64,
    /// Duration of grant pool inflation, in years
    pub grant_term: f64,

    /// Percentage of total inflation allocated to replicator rewards
    pub replicator: f64,
}

const DEFAULT_INITIAL: f64 = 0.15;
const DEFAULT_TERMINAL: f64 = 0.015;
const DEFAULT_TAPER: f64 = 0.15;
const DEFAULT_FOUNDATION: f64 = 0.05;
const DEFAULT_GRANT: f64 = 0.05;
const DEFAULT_FOUNDATION_GRANT_TERM: f64 = 7.0;
const DEFAULT_REPLICATOR: f64 = 0.10;

impl Default for Inflation {
    fn default() -> Self {
        Self {
            initial: DEFAULT_INITIAL,
            terminal: DEFAULT_TERMINAL,
            taper: DEFAULT_TAPER,
            foundation: DEFAULT_FOUNDATION,
            foundation_term: DEFAULT_FOUNDATION_GRANT_TERM,
            grant: DEFAULT_GRANT,
            grant_term: DEFAULT_FOUNDATION_GRANT_TERM,
            replicator: DEFAULT_REPLICATOR,
        }
    }
}

impl Inflation {
    /// inflation rate at year
    pub fn total(&self, year: f64) -> f64 {
        let tapered = self.initial * ((1.0 - self.taper).powf(year));

        if tapered > self.terminal {
            tapered
        } else {
            self.terminal
        }
    }

    /// portion of total that goes to validators
    pub fn validator(&self, year: f64) -> f64 {
        self.total(year) - self.replicator(year) - self.grant(year) - self.foundation(year)
    }

    /// portion of total that goes to replicators
    pub fn replicator(&self, year: f64) -> f64 {
        self.total(year) * self.replicator
    }

    /// portion of total that goes to grant pools
    pub fn grant(&self, year: f64) -> f64 {
        if year < self.grant_term {
            self.total(year) * self.grant
        } else {
            0.0
        }
    }

    /// portion of total that goes to foundation
    pub fn foundation(&self, year: f64) -> f64 {
        if year < self.foundation_term {
            self.total(year) * self.foundation
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inflation_basic() {
        let inflation = Inflation::default();

        let mut last = inflation.total(0.0);

        for year in &[0.1, 0.5, 1.0, DEFAULT_FOUNDATION_GRANT_TERM, 100.0] {
            let total = inflation.total(*year);
            assert_eq!(
                total,
                inflation.validator(*year)
                    + inflation.replicator(*year)
                    + inflation.grant(*year)
                    + inflation.foundation(*year)
            );
            assert!(total < last);
            assert!(total >= inflation.terminal);
            last = total;
        }
        assert_eq!(last, inflation.terminal);
    }
}
