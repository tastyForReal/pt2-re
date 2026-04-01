use std::collections::HashMap;

/// Bitmap font data structures and parser, ported from TypeScript.
#[derive(Debug, Clone)]
pub struct BitmapFontCharacter {
    pub id: u32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub x_offset: i32,
    pub y_offset: i32,
    pub x_advance: i32,
    pub page: i32,
}

#[derive(Debug, Clone)]
pub struct BitmapFontCommon {
    pub line_height: i32,
    pub base: i32,
    pub scale_w: i32,
    pub scale_h: i32,
    pub pages: i32,
}

#[derive(Debug, Clone)]
pub struct BitmapFontData {
    pub common: BitmapFontCommon,
    pub chars: HashMap<u32, BitmapFontCharacter>,
    pub page_file: String,
}

pub fn parse_bitmap_font(fnt_content: &str) -> BitmapFontData {
    let mut common = BitmapFontCommon {
        line_height: 0,
        base: 0,
        scale_w: 0,
        scale_h: 0,
        pages: 0,
    };
    let mut chars = HashMap::new();
    let mut page_file = String::new();

    for line in fnt_content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("common ") {
            common.line_height = extract_number(trimmed, "lineHeight");
            common.base = extract_number(trimmed, "base");
            common.scale_w = extract_number(trimmed, "scaleW");
            common.scale_h = extract_number(trimmed, "scaleH");
            common.pages = extract_number(trimmed, "pages");
        } else if trimmed.starts_with("page ") {
            page_file = extract_string(trimmed, "file");
        } else if trimmed.starts_with("char ") {
            let ch = BitmapFontCharacter {
                id: extract_number(trimmed, "id") as u32,
                x: extract_number(trimmed, "x"),
                y: extract_number(trimmed, "y"),
                width: extract_number(trimmed, "width"),
                height: extract_number(trimmed, "height"),
                x_offset: extract_number(trimmed, "xoffset"),
                y_offset: extract_number(trimmed, "yoffset"),
                x_advance: extract_number(trimmed, "xadvance"),
                page: extract_number(trimmed, "page"),
            };
            chars.insert(ch.id, ch);
        }
    }

    BitmapFontData {
        common,
        chars,
        page_file,
    }
}

fn extract_number(line: &str, key: &str) -> i32 {
    let pattern = format!("{}=", key);
    if let Some(pos) = line.find(&pattern) {
        let start = pos + pattern.len();
        let rest = &line[start..];
        let end = rest
            .find(|c: char| !c.is_ascii_digit() && c != '-')
            .unwrap_or(rest.len());
        rest[..end].parse().unwrap_or(0)
    } else {
        0
    }
}

fn extract_string(line: &str, key: &str) -> String {
    let pattern = format!("{}=\"", key);
    if let Some(pos) = line.find(&pattern) {
        let start = pos + pattern.len();
        let rest = &line[start..];
        let end = rest.find('"').unwrap_or(rest.len());
        rest[..end].to_string()
    } else {
        String::new()
    }
}

pub fn calculate_text_width(text: &str, font: &BitmapFontData, scale: f32) -> f32 {
    let mut width = 0.0f32;
    for ch in text.chars() {
        let char_code = ch as u32;
        if let Some(info) = font.chars.get(&char_code) {
            width += info.x_advance as f32 * scale;
        }
    }
    width
}
