use super::midi_types::*;
use super::types::*;
use std::collections::HashMap;

/// Load a level from a parsed JSON value.
pub fn load_level_from_json(json: &serde_json::Value) -> Result<LevelData, String> {
    let data: RawMusicInputFile = serde_json::from_value(json.clone())
        .map_err(|e| format!("Invalid JSON structure: {}", e))?;
    load_level_from_raw(&data)
}

/// Load a level from a raw music input file.
pub fn load_level_from_raw(data: &RawMusicInputFile) -> Result<LevelData, String> {
    let mut all_rows = Vec::new();
    let mut musics_metadata = Vec::new();
    let mut current_row_index = 0usize;

    for music in &data.musics {
        let rows = process_music(music);
        let bpm = music.bpm.unwrap_or(data.base_bpm);
        let tps = bpm / music.base_beats / 60.0;

        musics_metadata.push(MusicMetadata {
            id: music.id,
            tps: tps as f32,
            bpm,
            base_beats: music.base_beats,
            start_row_index: current_row_index,
            end_row_index: current_row_index + rows.len(),
            row_count: rows.len(),
        });

        all_rows.extend(rows);
        current_row_index = all_rows.len();
    }

    let midi_json = convert_raw_to_midi_json(&data.musics, data.base_bpm);

    Ok(LevelData {
        rows: all_rows,
        musics: musics_metadata,
        base_bpm: data.base_bpm,
        midi_json: Some(midi_json),
    })
}

// ---- Score parsing & row generation ----

static DURATION_MAP: &[(&str, i64)] = &[
    ("H", 256),
    ("I", 128),
    ("J", 64),
    ("K", 32),
    ("L", 16),
    ("M", 8),
    ("N", 4),
    ("O", 2),
    ("P", 1),
];

static REST_MAP: &[(&str, i64)] = &[
    ("Q", 256),
    ("R", 128),
    ("S", 64),
    ("T", 32),
    ("U", 16),
    ("V", 8),
    ("W", 4),
    ("X", 2),
    ("Y", 1),
];

fn duration_value(ch: char) -> Option<i64> {
    DURATION_MAP
        .iter()
        .find(|&&(c, _)| c.starts_with(ch))
        .map(|&(_, v)| v)
}

fn rest_value(ch: char) -> Option<i64> {
    REST_MAP
        .iter()
        .find(|&&(c, _)| c.starts_with(ch))
        .map(|&(_, v)| v)
}

fn extract_duration_letters(s: &str) -> i64 {
    s.chars().filter_map(duration_value).sum()
}

fn extract_rest_letters(s: &str) -> i64 {
    s.chars().filter_map(rest_value).sum()
}

fn extract_all_letters(s: &str) -> i64 {
    s.chars()
        .filter_map(|c| duration_value(c).or_else(|| rest_value(c)))
        .sum()
}

fn is_only_rest_letters(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| rest_value(c).is_some())
}

fn calculate_height_multiplier(duration: i64, divisor: i64) -> f32 {
    if duration <= divisor {
        1.0
    } else {
        duration as f32 / divisor as f32
    }
}

struct ParsedComponent {
    duration: i64,
    original_type: RowType,
}

fn parse_component(component: &str) -> ParsedComponent {
    let trimmed = component.trim();

    // Check for group pattern like 5<...>
    if trimmed.len() >= 3
        && let Some(first_char) = trimmed.chars().next()
        && first_char.is_ascii_digit()
        && trimmed.chars().nth(1) == Some('<')
        && trimmed.ends_with('>')
    {
        let type_id = first_char.to_digit(10).unwrap_or(0);
        let group_content = &trimmed[2..trimmed.len() - 1];
        let duration = extract_all_letters(group_content);
        let original_type = if type_id == 5 {
            RowType::DoubleTileRow
        } else {
            RowType::SingleTileRow
        };
        return ParsedComponent {
            duration,
            original_type,
        };
    }

    if is_only_rest_letters(trimmed) {
        let duration = extract_rest_letters(trimmed);
        return ParsedComponent {
            duration,
            original_type: RowType::EmptyRow,
        };
    }

    let duration = extract_duration_letters(trimmed);
    ParsedComponent {
        duration,
        original_type: RowType::SingleTileRow,
    }
}

fn split_score(score: &str) -> Vec<String> {
    let mut components = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = score.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];
        if ch == '<' {
            if !current.is_empty() && current.chars().last().is_some_and(|c| c.is_ascii_digit()) {
                let digit = current.pop().unwrap();
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() {
                    components.push(trimmed);
                }
                current.clear();

                let mut depth = 0;
                let mut group_str = String::new();
                group_str.push(digit);
                while i < chars.len() {
                    let c = chars[i];
                    if c == '<' {
                        depth += 1;
                    } else if c == '>' {
                        depth -= 1;
                    }
                    group_str.push(c);
                    i += 1;
                    if depth == 0 {
                        break;
                    }
                }
                components.push(group_str);
                if i < chars.len() && chars[i] == ',' {
                    i += 1;
                }
            } else {
                current.push(ch);
                i += 1;
            }
        } else if ch == ',' || ch == ';' {
            let trimmed = current.trim().to_string();
            if !trimmed.is_empty() {
                components.push(trimmed);
            }
            current.clear();
            i += 1;
        } else {
            current.push(ch);
            i += 1;
        }
    }

    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        components.push(trimmed);
    }
    components
}

fn parse_score(score: &str) -> Vec<ParsedComponent> {
    split_score(score)
        .iter()
        .map(|s| parse_component(s))
        .collect()
}

fn process_music(music: &RawMusicEntry) -> Vec<RowTypeResult> {
    let unit_divisor = (32.0 * music.base_beats) as i64;
    if music.scores.is_empty() {
        return Vec::new();
    }

    let primary_score = match music.scores.first() {
        Some(s) => s,
        None => return Vec::new(),
    };

    let primary_components = parse_score(primary_score);

    // Build primary timeline for blending
    let mut primary_timeline = Vec::new();
    let mut current_time = 0i64;
    for (i, comp) in primary_components.iter().enumerate() {
        primary_timeline.push((
            i,
            current_time,
            current_time + comp.duration,
            comp.original_type == RowType::EmptyRow,
        ));
        current_time += comp.duration;
    }

    // Blend secondary tracks
    let mut blended_indices = std::collections::HashSet::new();
    for secondary_score in music.scores.iter().skip(1) {
        let secondary_components = parse_score(secondary_score);
        let mut sec_time = 0i64;
        for sec_comp in &secondary_components {
            if sec_comp.original_type != RowType::EmptyRow {
                // Find primary entry
                for &(idx, start, end, is_rest) in &primary_timeline {
                    if sec_time >= start && sec_time < end && is_rest {
                        blended_indices.insert(idx);
                        break;
                    }
                }
            }
            sec_time += sec_comp.duration;
        }
    }

    primary_components
        .iter()
        .enumerate()
        .map(|(i, comp)| {
            if comp.original_type == RowType::EmptyRow {
                if blended_indices.contains(&i) {
                    RowTypeResult {
                        row_type: RowType::SingleTileRow,
                        height_multiplier: calculate_height_multiplier(comp.duration, unit_divisor),
                    }
                } else {
                    RowTypeResult {
                        row_type: RowType::EmptyRow,
                        height_multiplier: comp.duration as f32 / unit_divisor as f32,
                    }
                }
            } else {
                let hm = if comp.original_type == RowType::DoubleTileRow {
                    1.0
                } else {
                    calculate_height_multiplier(comp.duration, unit_divisor)
                };
                RowTypeResult {
                    row_type: comp.original_type,
                    height_multiplier: hm,
                }
            }
        })
        .collect()
}

// ---- MIDI conversion ----

pub fn get_note_number(note_name: &str) -> Option<u8> {
    super::midi_types::note_to_midi(note_name)
}

pub fn get_base_beats_multiplier(base_beats_str: &str) -> Result<f64, String> {
    BASE_BEATS_MAP
        .get(base_beats_str)
        .copied()
        .ok_or_else(|| format!("Unknown base_beats value: {}", base_beats_str))
}

fn get_length(s: &str, base_beats: f64) -> i64 {
    let mut delay = 0i64;
    for ch in s.chars() {
        let val = match ch {
            'H' => 256,
            'I' => 128,
            'J' => 64,
            'K' => 32,
            'L' => 16,
            'M' => 8,
            'N' => 4,
            'O' => 2,
            'P' => 1,
            _ => return 0,
        };
        delay += (val as f64 * base_beats) as i64;
    }
    delay
}

fn get_rest(s: &str, base_beats: f64) -> i64 {
    let mut delay = 0i64;
    for ch in s.chars() {
        let val = match ch {
            'Q' => 256,
            'R' => 128,
            'S' => 64,
            'T' => 32,
            'U' => 16,
            'V' => 8,
            'W' => 4,
            'X' => 2,
            'Y' => 1,
            _ => return 0,
        };
        delay += (val as f64 * base_beats) as i64;
    }
    delay
}

struct SafeDivider {
    remainder: i64,
}

impl SafeDivider {
    fn new() -> Self {
        Self { remainder: 0 }
    }

    fn divide(&mut self, a: i64, b: i64) -> i64 {
        if b == 0 {
            return 0;
        }
        let c = a / b;
        self.remainder += a - c * b;
        if self.remainder >= b {
            self.remainder -= b;
            c + 1
        } else {
            c
        }
    }
}

fn parse_track_score(score: &str, bpm: f64, base_beats: f64) -> ParsedTrack {
    let mut messages = Vec::new();
    let mut mode = 0u8;
    let mut notes: Vec<i64> = Vec::new();
    let chars: Vec<char> = score.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];
        match ch {
            ' ' => {
                i += 1;
                continue;
            }
            '.' => {
                if mode == 2 {
                    mode = 1;
                } else { /* skip */
                }
                i += 1;
            }
            '~' | '$' => {
                if mode == 2 {
                    mode = 1;
                }
                notes.push(2);
                i += 1;
            }
            '@' => {
                if mode == 2 {
                    mode = 1;
                }
                notes.push(3);
                i += 1;
            }
            '%' => {
                if mode == 2 {
                    mode = 1;
                }
                notes.push(4);
                i += 1;
            }
            '!' => {
                if mode == 2 {
                    mode = 1;
                }
                notes.push(5);
                i += 1;
            }
            '^' | '&' => {
                if mode == 2 {
                    mode = 1;
                }
                notes.push(6);
                i += 1;
            }
            '(' => {
                if mode == 0 {
                    mode = 1;
                }
                i += 1;
            }
            ')' => {
                if mode == 2 {
                    mode = 3;
                }
                i += 1;
            }
            '[' => {
                if mode == 3 {
                    mode = 4;
                }
                i += 1;
            }
            ']' => {
                if mode == 6 {
                    mode = 5;
                }
                i += 1;
            }
            ',' | ';' => {
                if mode == 5 {
                    mode = 0;
                } else if mode == 0 {
                    // Valid skip
                } else {
                    log::error!(
                        "Unexpected semicolon or comma in MIDI score (mode {})",
                        mode
                    );
                }
                i += 1;
            }
            _ => {
                if (ch == '<' || ch.is_ascii_digit()) && mode == 0 {
                    i += 1;
                    continue;
                }
                let la = chars.get(i).copied();
                if let Some(la_ch) = la
                    && (la_ch == '>' || la_ch == '{' || la_ch == '}' || la_ch.is_ascii_digit())
                    && mode == 5
                {
                    i += 1;
                    continue;
                }

                // Collect token
                let mut temp = String::new();
                loop {
                    if i >= chars.len() {
                        break;
                    }
                    temp.push(chars[i]);
                    i += 1;
                    if i >= chars.len() {
                        break;
                    }
                    let la = chars[i];
                    if matches!(
                        la,
                        '.' | '('
                            | ')'
                            | '~'
                            | '['
                            | ']'
                            | ','
                            | ';'
                            | '<'
                            | '>'
                            | '@'
                            | '%'
                            | '!'
                            | '$'
                            | '^'
                            | '&'
                    ) {
                        break;
                    }
                }

                let note = get_note_number(&temp).map(|v| v as i64).unwrap_or(0);
                let length = get_length(&temp, base_beats);
                let rest = get_rest(&temp, base_beats);

                if note > 0 {
                    if mode == 0 {
                        mode = 3;
                    } else if mode == 1 {
                        mode = 2;
                    } else {
                        log::error!("Unexpected token '{}' in MIDI score (mode {})", temp, mode);
                    }
                    if note != 1 {
                        notes.push(note);
                    }
                } else if length > 0 {
                    if mode != 4 {
                        log::error!(
                            "Length token '{}' found without opening '[' in MIDI score (mode {})",
                            temp,
                            mode
                        );
                    }
                    mode = 6;
                    process_notes(&notes, length, &mut messages, bpm);
                    notes.clear();
                } else if rest > 0 {
                    if mode == 0 {
                        mode = 5;
                        messages.push(ParsedMessage {
                            msg_type: 2,
                            value: rest,
                        });
                    } else if mode == 1 {
                        mode = 2;
                    } else {
                        log::error!(
                            "Rest token '{}' found in unexpected mode {} in MIDI score",
                            temp,
                            mode
                        );
                    }
                } else {
                    log::error!("Couldn't parse MIDI token '{}' in mode {}", temp, mode);
                }
            }
        }
    }

    if mode != 0 && mode != 5 {
        log::error!("Incomplete MIDI score string (final mode {})", mode);
    }

    ParsedTrack {
        base_beats,
        messages,
    }
}

fn process_notes(notes: &[i64], mut length: i64, messages: &mut Vec<ParsedMessage>, bpm: f64) {
    let mut sdiv = SafeDivider::new();
    let div_count = notes.iter().filter(|&&n| n == 2).count() as i64;
    let arp1 = notes.iter().filter(|&&n| n == 3).count() as i64;
    let arp2 = notes.iter().filter(|&&n| n == 4).count() as i64;
    let arp3 = notes.iter().filter(|&&n| n == 5).count() as i64;
    let arp4 = notes.iter().filter(|&&n| n == 6).count() as i64;

    let op_count = (div_count > 0) as i64
        + (arp1 > 0) as i64
        + (arp2 > 0) as i64
        + (arp3 > 0) as i64
        + (arp4 > 0) as i64;
    if op_count > 1 || arp4 > 1 {
        log::error!("Problem with operators in MIDI parsing");
        return;
    }

    let divisor = div_count + 1;

    if arp1 > 0 {
        for idx in 0..=notes.len() {
            if idx == notes.len() {
                messages.push(ParsedMessage {
                    msg_type: 2,
                    value: length,
                });
                for &n in notes {
                    if n != 3 {
                        messages.push(ParsedMessage {
                            msg_type: 1,
                            value: n,
                        });
                    }
                }
            } else {
                let note_val = notes[idx];
                if note_val == 3 {
                    let delay = if arp1 == 1 {
                        sdiv.divide(length, 10)
                    } else {
                        sdiv.divide(length, 10 * (arp1 - 1))
                    };
                    length -= delay;
                    messages.push(ParsedMessage {
                        msg_type: 2,
                        value: delay,
                    });
                } else {
                    messages.push(ParsedMessage {
                        msg_type: 0,
                        value: note_val,
                    });
                }
            }
        }
    } else if arp2 > 0 {
        for idx in 0..=notes.len() {
            if idx == notes.len() {
                messages.push(ParsedMessage {
                    msg_type: 2,
                    value: length,
                });
                for &n in notes {
                    if n != 4 {
                        messages.push(ParsedMessage {
                            msg_type: 1,
                            value: n,
                        });
                    }
                }
            } else {
                let note_val = notes[idx];
                if note_val == 4 {
                    let delay = sdiv.divide(3 * length, 10 * arp2);
                    length -= delay;
                    messages.push(ParsedMessage {
                        msg_type: 2,
                        value: delay,
                    });
                } else {
                    messages.push(ParsedMessage {
                        msg_type: 0,
                        value: note_val,
                    });
                }
            }
        }
    } else if arp3 > 0 {
        for idx in 0..=notes.len() {
            if idx == notes.len() {
                messages.push(ParsedMessage {
                    msg_type: 2,
                    value: length,
                });
                for &n in notes {
                    if n != 5 {
                        messages.push(ParsedMessage {
                            msg_type: 1,
                            value: n,
                        });
                    }
                }
            } else {
                let note_val = notes[idx];
                if note_val == 5 {
                    let delay = sdiv.divide(3 * length, 20 * arp3);
                    length -= delay;
                    messages.push(ParsedMessage {
                        msg_type: 2,
                        value: delay,
                    });
                } else {
                    messages.push(ParsedMessage {
                        msg_type: 0,
                        value: note_val,
                    });
                }
            }
        }
    } else if arp4 > 0 {
        if notes.len() != 3 || notes[1] != 6 || notes[0] < 20 || notes[2] < 20 {
            log::error!("Problem with ornament: invalid notes array length or values");
            return;
        }

        let mut note_flip = 0;
        let bpm32 = (bpm * 32.0) as i64;

        loop {
            let current_note = notes[note_flip];
            messages.push(ParsedMessage {
                msg_type: 0,
                value: current_note,
            });

            let delay = sdiv.divide(bpm32, 720);
            if delay >= length {
                messages.push(ParsedMessage {
                    msg_type: 2,
                    value: length,
                });
                messages.push(ParsedMessage {
                    msg_type: 1,
                    value: notes[note_flip],
                });
                break;
            } else {
                length -= delay;
                messages.push(ParsedMessage {
                    msg_type: 2,
                    value: delay,
                });
                messages.push(ParsedMessage {
                    msg_type: 1,
                    value: notes[note_flip],
                });
            }

            if note_flip == 0 {
                note_flip = 2;
            } else {
                note_flip = 0;
            }
        }
    } else {
        let mut temp_notes = Vec::new();

        for idx in 0..=notes.len() {
            let note_val = notes.get(idx).copied();
            if idx == notes.len() || note_val == Some(2) {
                for &tn in &temp_notes {
                    messages.push(ParsedMessage {
                        msg_type: 0,
                        value: tn,
                    });
                }
                messages.push(ParsedMessage {
                    msg_type: 2,
                    value: sdiv.divide(length, divisor),
                });
                for &tn in &temp_notes {
                    messages.push(ParsedMessage {
                        msg_type: 1,
                        value: tn,
                    });
                }
                temp_notes.clear();
            } else if let Some(nv) = note_val {
                temp_notes.push(nv);
            }
        }
    }
}

fn calculate_track_length_diff(messages1: &[ParsedMessage], messages2: &[ParsedMessage]) -> i64 {
    let mut diff = 0i64;
    let mut msg1 = messages1.to_vec();
    let mut msg2 = messages2.to_vec();
    let mut a = 0;
    let mut b = 0;

    while a < msg1.len() || b < msg2.len() {
        while a < msg1.len() {
            if msg1[a].msg_type == 2 && msg1[a].value != 0 {
                msg1[a].value -= 1;
                diff += 1;
                break;
            } else {
                a += 1;
            }
        }
        while b < msg2.len() {
            if msg2[b].msg_type == 2 && msg2[b].value != 0 {
                msg2[b].value -= 1;
                diff -= 1;
                break;
            } else {
                b += 1;
            }
        }
    }
    diff
}

fn shrink_track(messages: &mut [ParsedMessage], amount: i64) {
    let mut remaining = amount;
    for i in (0..messages.len()).rev() {
        if remaining <= 0 {
            break;
        }
        if messages[i].msg_type == 2 {
            let diff = remaining - messages[i].value;
            if diff >= 0 {
                remaining = diff;
                messages[i].value = 0;
            } else {
                messages[i].value = -diff;
                remaining = 0;
            }
        }
    }

    let mut note_on_stack = Vec::new();
    for i in 0..messages.len() {
        if messages[i].msg_type == 2 && messages[i].value > 0 {
            note_on_stack.clear();
        } else if messages[i].msg_type == 0 {
            note_on_stack.push(i);
        } else if messages[i].msg_type == 1 {
            let note_val = messages[i].value;
            if let Some(pos) = note_on_stack
                .iter()
                .position(|&idx| messages[idx].value == note_val)
            {
                let on_idx = note_on_stack.remove(pos);
                // In TS this sets type to 3... let's follow.
                messages[i].msg_type = 3;
                messages[i].value = 0;
                messages[on_idx].msg_type = 3;
                messages[on_idx].value = 0;
            }
        }
    }
}

fn verify_track_length(tracks: &mut [ParsedTrack]) {
    if tracks.is_empty() {
        return;
    }
    let (first, others) = tracks.split_at_mut(1);
    let track0 = &first[0];

    for track_i in others {
        let diff = calculate_track_length_diff(&track0.messages, &track_i.messages);
        if diff < 0 {
            shrink_track(&mut track_i.messages, -diff);
        } else if diff > 0 {
            track_i.messages.push(ParsedMessage {
                msg_type: 2,
                value: diff,
            });
        }
    }
}

fn parse_song(musics: &[RawMusicEntry], base_bpm: f64) -> Vec<ParsedPart> {
    let mut parts = Vec::new();
    for music in musics {
        let base_beats_str = format!("{}", music.base_beats);
        let bbm = get_base_beats_multiplier(&base_beats_str).unwrap_or(1.0);
        let music_bpm = music.bpm.unwrap_or(base_bpm);
        let calculated_bpm = music_bpm * bbm;

        let mut part = ParsedPart {
            bpm: if calculated_bpm > 0.0 {
                calculated_bpm
            } else {
                120.0 * bbm
            },
            base_beats: bbm,
            tracks: Vec::new(),
        };

        for score in &music.scores {
            let track = parse_track_score(score, part.bpm, bbm);
            part.tracks.push(track);
        }

        verify_track_length(&mut part.tracks);
        parts.push(part);
    }
    parts
}

fn convert_to_midi_json(parts: &[ParsedPart]) -> MidiJson {
    let ppq = 960u32;
    let mut tempos = Vec::new();
    let mut tracks: Vec<MidiTrack> = Vec::new();
    let mut current_ticks = 0i64;

    for part in parts {
        let actual_bpm = part.bpm / 30.0;
        tempos.push(MidiTempo {
            ticks: current_ticks,
            bpm: actual_bpm,
            time: None,
        });

        for (track_idx, track) in part.tracks.iter().enumerate() {
            while tracks.len() <= track_idx {
                tracks.push(MidiTrack {
                    name: None,
                    channel: Some((tracks.len() % 16) as u8),
                    notes: Vec::new(),
                });
            }
            let output_track = &mut tracks[track_idx];
            let mut tick_pos = current_ticks;
            let mut active_notes: HashMap<i64, i64> = HashMap::new();

            for msg in &track.messages {
                match msg.msg_type {
                    0 => {
                        active_notes.insert(msg.value, tick_pos);
                    }
                    1 => {
                        if let Some(&note_start) = active_notes.get(&msg.value) {
                            output_track.notes.push(MidiNote {
                                midi: msg.value as u8,
                                name: None,
                                ticks: note_start,
                                time: 0.0,
                                duration: 0.0,
                                duration_ticks: tick_pos - note_start,
                                velocity: 100.0 / 127.0,
                                note_off_velocity: 64.0 / 127.0,
                            });
                            active_notes.remove(&msg.value);
                        }
                    }
                    2 => {
                        tick_pos += msg.value;
                    }
                    _ => {}
                }
            }
        }

        // Calculate part duration
        let mut max_dur = 0i64;
        for track in &part.tracks {
            let dur: i64 = track
                .messages
                .iter()
                .filter(|m| m.msg_type == 2)
                .map(|m| m.value)
                .sum();
            if dur > max_dur {
                max_dur = dur;
            }
        }
        current_ticks += max_dur;
    }

    // Calculate times
    tempos.sort_by_key(|t| t.ticks);
    for track in &mut tracks {
        for note in &mut track.notes {
            note.time = ticks_to_seconds(note.ticks, &tempos, ppq);
            note.duration =
                ticks_to_seconds(note.ticks + note.duration_ticks, &tempos, ppq) - note.time;
        }
    }
    let tempo_ticks: Vec<i64> = tempos.iter().map(|t| t.ticks).collect();
    let tempos_snapshot = tempos.clone();
    for (i, ticks) in tempo_ticks.iter().enumerate() {
        tempos[i].time = Some(ticks_to_seconds(*ticks, &tempos_snapshot, ppq));
    }

    MidiJson {
        header: MidiHeader {
            ppq,
            tempos,
            name: None,
        },
        tracks,
    }
}

fn align_tracks_across_parts(parts: &mut [ParsedPart]) {
    let mut max_tracks = 0;
    for part in parts.iter() {
        if part.tracks.len() > max_tracks {
            max_tracks = part.tracks.len();
        }
    }

    for part in parts.iter_mut() {
        while part.tracks.len() < max_tracks {
            let last_track = match part.tracks.last() {
                Some(t) => t,
                None => break,
            };
            let mut new_messages = Vec::new();
            for msg in &last_track.messages {
                if msg.msg_type < 2 {
                    new_messages.push(ParsedMessage {
                        msg_type: 3,
                        value: msg.value,
                    });
                } else {
                    new_messages.push(ParsedMessage {
                        msg_type: msg.msg_type,
                        value: msg.value,
                    });
                }
            }
            part.tracks.push(ParsedTrack {
                base_beats: last_track.base_beats,
                messages: new_messages,
            });
        }
    }
}

pub fn convert_raw_to_midi_json(musics: &[RawMusicEntry], base_bpm: f64) -> MidiJson {
    let mut parts = parse_song(musics, base_bpm);
    align_tracks_across_parts(&mut parts);
    convert_to_midi_json(&parts)
}
