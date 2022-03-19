use serde::Deserialize;

fn one() -> f32 {
    1.
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LessonConfig {
    #[serde(default = "one")]
    pub scale: f32,

    #[serde(default)]
    pub ignore_colors: Vec<Vec<u8>>,
}

impl Default for LessonConfig {
    fn default() -> Self {
        Self { scale: 1., ignore_colors: Vec::new() }
    }
}
