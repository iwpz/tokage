//! Shared user choices. Token limits are visible UTF-8 byte/4 budgets, not tokenizer measurements.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextTuning {
    pub brief_token_limit: u32,
    pub report_token_limit: u32,
    pub rework_phi: f64,
    pub calibration_min_samples: u32,
}
impl Default for ContextTuning {
    fn default() -> Self {
        Self {
            brief_token_limit: 3000,
            report_token_limit: 300,
            rework_phi: 0.3,
            calibration_min_samples: 5,
        }
    }
}
impl ContextTuning {
    pub fn validate(&self) -> Result<(), String> {
        if !(256..=32000).contains(&self.brief_token_limit)
            || self.report_token_limit < 32
            || self.report_token_limit > self.brief_token_limit
            || !self.rework_phi.is_finite()
            || !(0.0..=1.0).contains(&self.rework_phi)
            || !(3..=100).contains(&self.calibration_min_samples)
        {
            return Err(
                "Invalid context tuning limits, rework coefficient or minimum sample count".into(),
            );
        }
        Ok(())
    }
    pub fn clip_report(&self, text: &str) -> String {
        let max = self.report_token_limit as usize * 4;
        if text.len() <= max {
            return text.into();
        }
        let marker = "\n[truncated excerpt]";
        let mut end = max.saturating_sub(marker.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}{}", &text[..end], marker)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zero_phi_is_valid_and_utf8_excerpt_budgets_are_bounded() {
        let mut t = ContextTuning::default();
        t.rework_phi = 0.;
        assert!(t.validate().is_ok());
        let excerpt = t.clip_report(&"合同と証拠 🐟 ".repeat(1000));
        assert!(excerpt.len() <= 1200);
        assert!(excerpt.ends_with("[truncated excerpt]"));
        t.report_token_limit = t.brief_token_limit + 1;
        assert!(t.validate().is_err());
        t = Default::default();
        t.rework_phi = f64::NAN;
        assert!(t.validate().is_err());
    }
}
