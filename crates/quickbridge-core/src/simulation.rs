/// Built-in simulation scenarios used for tests and local development.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SimulationScenario {
    HappyPath,
    NoRanges,
}

impl SimulationScenario {
    pub fn label(&self) -> &'static str {
        match self {
            Self::HappyPath => "happy-path",
            Self::NoRanges => "no-ranges",
        }
    }
}
