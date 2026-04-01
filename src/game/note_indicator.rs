use super::midi_types::MidiJson;
use super::types::*;

const INDICATOR_SIZE: f32 = 16.0;
const INDICATOR_Y_OFFSET: f32 = -8.0;

pub fn build_note_indicators(
    midi_json: &MidiJson,
    rows: &[RowData],
    musics_metadata: &[MusicMetadata],
) -> Vec<NoteIndicatorData> {
    let mut indicators = Vec::new();
    if midi_json.tracks.is_empty() || rows.is_empty() {
        return indicators;
    }

    let mut all_notes: Vec<(f64, i64, usize, u8)> = Vec::new();
    for (track_idx, track) in midi_json.tracks.iter().enumerate() {
        for note in &track.notes {
            let note_id = (note.time * 1000.0).round() as i64 * 1_000_000
                + track_idx as i64 * 1000
                + note.midi as i64;
            all_notes.push((note.time, note_id, track_idx, note.midi));
        }
    }
    all_notes.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    // Build row timing info
    let mut level_row_times = Vec::new();
    let mut cumulative_time = 0.0_f64;
    for (i, row) in rows.iter().enumerate().skip(1) {
        let level_row_index = row.row_index.wrapping_sub(1);
        let mut tps = DEFAULT_TPS as f64;
        for music in musics_metadata {
            if level_row_index >= music.start_row_index && level_row_index < music.end_row_index {
                tps = music.tps as f64;
                break;
            }
        }
        let row_start = cumulative_time;
        let time_per_base = 1.0 / tps;
        let row_duration = row.height_multiplier as f64 * time_per_base;
        let row_end = cumulative_time + row_duration;
        level_row_times.push((i, row_start, row_end));
        cumulative_time = row_end;
    }

    for &(note_time, note_id, track_idx, midi) in &all_notes {
        if !(21..=108).contains(&midi) {
            continue;
        }
        let mut target = None;
        for &(row_idx, start, end) in &level_row_times {
            if note_time >= start && note_time < end {
                target = Some((row_idx, start, end));
                break;
            }
        }
        let (row_idx, start, end) = match target {
            Some(t) => t,
            None => continue,
        };
        let row = &rows[row_idx];
        if row.row_type == RowType::StartingTileRow || row.tiles.is_empty() {
            continue;
        }
        let frac = (note_time - start) / (end - start);
        let row_bottom = row.y_position + row.height;
        let base_edge = row_bottom - BASE_ROW_HEIGHT;
        let indicator_y = base_edge - frac as f32 * row.height + INDICATOR_Y_OFFSET;
        let indicator_x = (SCREEN_WIDTH - INDICATOR_SIZE) / 2.0;

        indicators.push(NoteIndicatorData {
            note_id,
            row_index: row_idx,
            x: indicator_x,
            y: indicator_y,
            width: INDICATOR_SIZE,
            height: INDICATOR_SIZE,
            time: note_time,
            time_fraction: Some(frac),
            track_idx: Some(track_idx),
            midi: Some(midi),
            is_consumed: false,
        });
    }
    log::info!(
        "Built {} indicators from {} notes",
        indicators.len(),
        all_notes.len()
    );
    indicators
}

pub fn get_active_indicators(indicators: &[NoteIndicatorData]) -> Vec<&NoteIndicatorData> {
    indicators.iter().filter(|i| !i.is_consumed).collect()
}
