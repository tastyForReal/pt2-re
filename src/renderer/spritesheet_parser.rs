/// Spritesheet parser for .plist files.
/// Ported from the TypeScript version using roxmltree instead of DOMParser.

#[derive(Debug, Clone)]
pub struct IntRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Debug, Clone)]
pub struct IntPoint {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone)]
pub struct SpriteFrame {
    pub frame: IntRect,
    pub offset: IntPoint,
    pub rotated: bool,
    pub source_color_rect: IntRect,
    pub source_size: IntPoint,
}

#[derive(Debug, Clone)]
pub struct SpritesheetData {
    pub frames: std::collections::HashMap<String, SpriteFrame>,
    pub meta_image: String,
    pub meta_size: IntPoint,
}

impl SpritesheetData {
    pub fn get_sprite_size(&self, name: &str) -> Option<IntPoint> {
        self.frames.get(name).map(|f| f.source_size.clone())
    }
}

fn parse_vec2(s: &str) -> IntPoint {
    let re = regex_lite::Regex::new(r"\{(-?\d+),(-?\d+)\}").unwrap();
    if let Some(caps) = re.captures(s) {
        IntPoint {
            x: caps[1].parse().unwrap_or(0),
            y: caps[2].parse().unwrap_or(0),
        }
    } else {
        IntPoint { x: 0, y: 0 }
    }
}

fn parse_rect(s: &str) -> IntRect {
    let re = regex_lite::Regex::new(r"\{\{(-?\d+),(-?\d+)\},\{(-?\d+),(-?\d+)\}\}").unwrap();
    if let Some(caps) = re.captures(s) {
        IntRect {
            x: caps[1].parse().unwrap_or(0),
            y: caps[2].parse().unwrap_or(0),
            w: caps[3].parse().unwrap_or(0),
            h: caps[4].parse().unwrap_or(0),
        }
    } else {
        IntRect {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
        }
    }
}

/// Parse a plist spritesheet file.
pub fn parse_spritesheet(plist_content: &str) -> Result<SpritesheetData, String> {
    // roxmltree does not support DTDs, which are common in .plist files.
    // Strip the DOCTYPE declaration if it exists.
    let cleaned_content = if let Some(start) = plist_content.find("<!DOCTYPE") {
        if let Some(end) = plist_content[start..].find(">") {
            let mut s = plist_content.to_string();
            s.replace_range(start..start + end + 1, "");
            s
        } else {
            plist_content.to_string()
        }
    } else {
        plist_content.to_string()
    };

    let doc = roxmltree::Document::parse(&cleaned_content)
        .map_err(|e| format!("Failed to parse plist: {}", e))?;

    let mut data = SpritesheetData {
        frames: std::collections::HashMap::new(),
        meta_image: String::new(),
        meta_size: IntPoint { x: 1024, y: 1024 },
    };

    // Find root dict
    let root = doc.root_element();
    let root_dict = root.children().find(|n| n.has_tag_name("dict"));
    let root_dict = match root_dict {
        Some(d) => d,
        None => return Err("No root dict found".into()),
    };

    let children: Vec<_> = root_dict.children().filter(|n| n.is_element()).collect();
    let mut i = 0;
    while i < children.len() {
        let el = children[i];
        if el.has_tag_name("key") {
            let key_name = el.text().unwrap_or("").trim();
            if key_name == "frames" {
                if let Some(frames_dict) = children.get(i + 1) {
                    parse_frames(frames_dict, &mut data.frames);
                }
            } else if key_name == "metadata" {
                if let Some(meta_dict) = children.get(i + 1) {
                    parse_metadata(meta_dict, &mut data);
                }
            }
        }
        i += 1;
    }

    log::info!(
        "Parsed {} frames. Image: {}, Size: {}x{}",
        data.frames.len(),
        data.meta_image,
        data.meta_size.x,
        data.meta_size.y
    );
    Ok(data)
}

fn parse_frames(
    dict_node: &roxmltree::Node,
    frames: &mut std::collections::HashMap<String, SpriteFrame>,
) {
    let children: Vec<_> = dict_node.children().filter(|n| n.is_element()).collect();
    let keys: Vec<_> = children.iter().filter(|n| n.has_tag_name("key")).collect();
    let dicts: Vec<_> = children.iter().filter(|n| n.has_tag_name("dict")).collect();

    for (i, key_node) in keys.iter().enumerate() {
        let name = key_node.text().unwrap_or("").trim().to_string();
        if let Some(dict) = dicts.get(i) {
            let frame = parse_frame_dict(dict);
            frames.insert(name, frame);
        }
    }
}

fn parse_frame_dict(dict: &roxmltree::Node) -> SpriteFrame {
    let children: Vec<_> = dict.children().filter(|n| n.is_element()).collect();
    let mut frame_rect = IntRect {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
    };
    let mut offset = IntPoint { x: 0, y: 0 };
    let mut rotated = false;
    let mut source_size = IntPoint { x: 0, y: 0 };

    let mut j = 0;
    while j < children.len() {
        if children[j].has_tag_name("key") {
            let k = children[j].text().unwrap_or("").trim();
            if let Some(val) = children.get(j + 1) {
                match k {
                    "frame" | "textureRect" => {
                        frame_rect = parse_rect(val.text().unwrap_or(""));
                    }
                    "offset" | "spriteOffset" => {
                        offset = parse_vec2(val.text().unwrap_or(""));
                    }
                    "rotated" | "textureRotated" => {
                        rotated = val.has_tag_name("true");
                    }
                    "sourceSize" | "spriteSize" | "spriteSourceSize" => {
                        source_size = parse_vec2(val.text().unwrap_or(""));
                    }
                    _ => {}
                }
            }
        }
        j += 1;
    }

    if rotated {
        std::mem::swap(&mut frame_rect.w, &mut frame_rect.h);
    }

    if source_size.x == 0 && source_size.y == 0 {
        source_size = IntPoint {
            x: frame_rect.w,
            y: frame_rect.h,
        };
    }

    SpriteFrame {
        frame: frame_rect.clone(),
        offset,
        rotated,
        source_color_rect: IntRect {
            x: 0,
            y: 0,
            w: frame_rect.w,
            h: frame_rect.h,
        },
        source_size,
    }
}

fn parse_metadata(dict_node: &roxmltree::Node, data: &mut SpritesheetData) {
    let children: Vec<_> = dict_node.children().filter(|n| n.is_element()).collect();
    let mut j = 0;
    while j < children.len() {
        if children[j].has_tag_name("key") {
            let k = children[j].text().unwrap_or("").trim();
            if let Some(val) = children.get(j + 1) {
                match k {
                    "textureFileName" | "realTextureFileName" => {
                        data.meta_image = val.text().unwrap_or("").trim().to_string();
                    }
                    "size" => {
                        data.meta_size = parse_vec2(val.text().unwrap_or(""));
                    }
                    _ => {}
                }
            }
        }
        j += 1;
    }
}
