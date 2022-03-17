use serde::Deserialize;

fn one() -> f32 {
    1.
}

#[derive(Deserialize)]
pub(crate) struct LessonConfig {
    #[serde(default = "one")]
    pub scale: f32,
}

impl Default for LessonConfig {
    fn default() -> Self {
        Self { scale: 1. }
    }
}
