use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub bar_height: u16,
    pub spacing: u16,
    pub ratio: f32,
    pub border_size: u16,
    pub main_color: String,
    pub secondary_color: String,
    pub hotkeys: Vec<String>,
}

impl Config {
    pub fn new() -> Self {
        toml::from_str(&std::fs::read_to_string("config.toml").unwrap()).unwrap()
    }
}
