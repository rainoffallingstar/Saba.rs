use serde::{Deserialize, Serialize};

pub const THEME_TOKEN_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThemeColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl ThemeColor {
    pub fn rgb_u32(self) -> u32 {
        ((self.red as u32) << 16) | ((self.green as u32) << 8) | (self.blue as u32)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeTokens {
    pub schema_version: u32,
    pub board_wood: String,
    pub board_line: String,
    pub star_point: String,
    pub stone_black: String,
    pub stone_white: String,
    pub background: String,
}

impl ThemeTokens {
    pub fn parse(json: &str) -> Result<Self, String> {
        let tokens: Self =
            serde_json::from_str(json).map_err(|error| format!("invalid theme tokens: {error}"))?;
        if tokens.schema_version != THEME_TOKEN_SCHEMA_VERSION {
            return Err(format!(
                "unsupported theme-token schema version {}",
                tokens.schema_version
            ));
        }
        for color in [
            &tokens.board_wood,
            &tokens.board_line,
            &tokens.star_point,
            &tokens.stone_black,
            &tokens.stone_white,
            &tokens.background,
        ] {
            parse_hex_color(color)?;
        }
        Ok(tokens)
    }

    pub fn board_wood_color(&self) -> ThemeColor {
        parse_hex_color(&self.board_wood).expect("theme tokens were validated on parse")
    }

    pub fn board_line_color(&self) -> ThemeColor {
        parse_hex_color(&self.board_line).expect("theme tokens were validated on parse")
    }

    pub fn star_point_color(&self) -> ThemeColor {
        parse_hex_color(&self.star_point).expect("theme tokens were validated on parse")
    }

    pub fn stone_black_color(&self) -> ThemeColor {
        parse_hex_color(&self.stone_black).expect("theme tokens were validated on parse")
    }

    pub fn stone_white_color(&self) -> ThemeColor {
        parse_hex_color(&self.stone_white).expect("theme tokens were validated on parse")
    }

    pub fn background_color(&self) -> ThemeColor {
        parse_hex_color(&self.background).expect("theme tokens were validated on parse")
    }
}

impl Default for ThemeTokens {
    fn default() -> Self {
        Self {
            schema_version: THEME_TOKEN_SCHEMA_VERSION,
            board_wood: "#d9a866".to_owned(),
            board_line: "#4a2f12".to_owned(),
            star_point: "#3a2410".to_owned(),
            stone_black: "#1a1a1a".to_owned(),
            stone_white: "#ffffff".to_owned(),
            background: "#f5f0e8".to_owned(),
        }
    }
}

pub fn parse_hex_color(input: &str) -> Result<ThemeColor, String> {
    let hex = input.strip_prefix('#').unwrap_or(input);
    if hex.len() != 6 || !hex.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(format!("{input:?} is not a #RRGGBB color"));
    }
    let value = u32::from_str_radix(hex, 16)
        .map_err(|error| format!("{input:?} is not a valid hex color: {error}"))?;
    Ok(ThemeColor {
        red: ((value >> 16) & 0xff) as u8,
        green: ((value >> 8) & 0xff) as u8,
        blue: (value & 0xff) as u8,
    })
}

#[cfg(test)]
mod tests {
    use super::{THEME_TOKEN_SCHEMA_VERSION, ThemeTokens, parse_hex_color};

    #[test]
    fn parses_and_validates_hex_colors() {
        assert_eq!(
            parse_hex_color("#aabbcc").unwrap(),
            super::ThemeColor {
                red: 0xaa,
                green: 0xbb,
                blue: 0xcc,
            }
        );
        assert!(parse_hex_color("zzz").is_err());
        assert!(parse_hex_color("#12").is_err());
    }

    #[test]
    fn rejects_unknown_theme_token_schema_versions() {
        let json = r##"{"schemaVersion":2,"boardWood":"#d9a866","boardLine":"#4a2f12","starPoint":"#3a2410","stoneBlack":"#1a1a1a","stoneWhite":"#ffffff","background":"#f5f0e8"}"##;
        assert!(ThemeTokens::parse(json).is_err());
    }

    #[test]
    fn parses_valid_tokens_and_exposes_semantic_colors() {
        let tokens = ThemeTokens::parse(
            r##"{"schemaVersion":1,"boardWood":"#d9a866","boardLine":"#4a2f12","starPoint":"#3a2410","stoneBlack":"#1a1a1a","stoneWhite":"#ffffff","background":"#f5f0e8"}"##,
        )
        .unwrap();

        assert_eq!(tokens.schema_version, THEME_TOKEN_SCHEMA_VERSION);
        assert_eq!(tokens.board_wood_color().rgb_u32(), 0xd9a866);
        assert_eq!(tokens.stone_white_color().rgb_u32(), 0xffffff);
    }

    #[test]
    fn default_tokens_are_valid() {
        let default = ThemeTokens::default();
        assert_eq!(default.schema_version, THEME_TOKEN_SCHEMA_VERSION);
        assert!(ThemeTokens::parse(&serde_json::to_string(&default).unwrap()).is_ok());
    }
}
