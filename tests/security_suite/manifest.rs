use serde::Deserialize;
use serde_json::Value;

const CORPUS: &str = include_str!("../../tools/security/adversarial-cases.json");

#[derive(Debug, Deserialize)]
pub struct Corpus {
    pub cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
pub struct Case {
    pub id: String,
    pub backend: String,
    pub operation: String,
    pub payload: Value,
    pub expected_decision: String,
    pub expected_state_unchanged: bool,
    pub controls: Vec<String>,
    #[serde(default = "implemented")]
    pub status: String,
}

fn implemented() -> String {
    "implemented".into()
}

pub fn load() -> Corpus {
    serde_json::from_str(CORPUS).expect("adversarial case corpus must be valid JSON")
}

pub fn implemented_for(backend: &str) -> Vec<Case> {
    load()
        .cases
        .into_iter()
        .filter(move |case| case.status == "implemented" && case.backend == backend)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::load;

    #[test]
    fn implemented_cases_have_reproducible_invariants() {
        let cases = load().cases;
        assert!(cases.iter().any(|case| case.status == "implemented"));
        assert!(cases
            .iter()
            .filter(|case| case.status == "implemented")
            .all(|case| case.expected_state_unchanged && !case.controls.is_empty()));
    }
}
