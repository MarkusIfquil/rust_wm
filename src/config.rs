use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub bar_height: u16,
    pub spacing: u16,
    pub ratio: f32,
    pub border_size: u16,
    pub main_color: String,
    pub secondary_color: String,
    pub hotkeys: Vec<Vec<String>>,
}

impl Config {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(toml::from_str(&std::fs::read_to_string("config.toml")?)?)
    }
    pub fn default() -> Self {
        Config {
            bar_height: 20,
            spacing: 10,
            ratio: 0.5,
            border_size: 1,
            main_color: String::from("#11111b"),
            secondary_color: String::from("#74c7ec"),
            hotkeys: vec![
                vec![
                    String::from("CONTROL|MOD"),
                    String::from("Return"),
                    String::from("spawn"),
                    String::from("alacritty"),
                ],
                vec![
                    String::from("MOD"),
                    String::from("q"),
                    String::from("exit"),
                    String::from(""),
                ],
                vec![
                    String::from("MOD"),
                    String::from("c"),
                    String::from("spawn"),
                    String::from("rofi -show drun"),
                ],
                vec![
                    String::from("MOD"),
                    String::from("1"),
                    String::from("switchtag"),
                    String::from("1"),
                ],
                vec![
                    String::from("MOD"),
                    String::from("2"),
                    String::from("switchtag"),
                    String::from("2"),
                ],
                vec![
                    String::from("MOD"),
                    String::from("3"),
                    String::from("switchtag"),
                    String::from("3"),
                ],
                vec![
                    String::from("MOD"),
                    String::from("4"),
                    String::from("switchtag"),
                    String::from("4"),
                ],
                vec![
                    String::from("MOD"),
                    String::from("5"),
                    String::from("switchtag"),
                    String::from("5"),
                ],
                vec![
                    String::from("MOD"),
                    String::from("6"),
                    String::from("switchtag"),
                    String::from("6"),
                ],
                vec![
                    String::from("MOD"),
                    String::from("7"),
                    String::from("switchtag"),
                    String::from("7"),
                ],
                vec![
                    String::from("MOD"),
                    String::from("8"),
                    String::from("switchtag"),
                    String::from("8"),
                ],
                vec![
                    String::from("MOD"),
                    String::from("9"),
                    String::from("switchtag"),
                    String::from("9"),
                ],
                vec![
                    String::from("MOD|SHIFT"),
                    String::from("1"),
                    String::from("movewindow"),
                    String::from("1"),
                ],
                vec![
                    String::from("MOD|SHIFT"),
                    String::from("2"),
                    String::from("movewindow"),
                    String::from("2"),
                ],
                vec![
                    String::from("MOD|SHIFT"),
                    String::from("3"),
                    String::from("movewindow"),
                    String::from("3"),
                ],
                vec![
                    String::from("MOD|SHIFT"),
                    String::from("4"),
                    String::from("movewindow"),
                    String::from("4"),
                ],
                vec![
                    String::from("MOD|SHIFT"),
                    String::from("5"),
                    String::from("movewindow"),
                    String::from("5"),
                ],
                vec![
                    String::from("MOD|SHIFT"),
                    String::from("6"),
                    String::from("movewindow"),
                    String::from("6"),
                ],
                vec![
                    String::from("MOD|SHIFT"),
                    String::from("7"),
                    String::from("movewindow"),
                    String::from("7"),
                ],
                vec![
                    String::from("MOD|SHIFT"),
                    String::from("8"),
                    String::from("movewindow"),
                    String::from("8"),
                ],
                vec![
                    String::from("MOD|SHIFT"),
                    String::from("9"),
                    String::from("movewindow"),
                    String::from("9"),
                ],
            ],
        }
    }
}
