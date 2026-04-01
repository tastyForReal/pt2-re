use super::audio_manager::AudioManager;
use super::row_generator;
use super::score_manager::ScoreManager;
use super::types::*;

pub fn update_scroll(
    data: &mut GameData,
    dt: f64,
    current_time: f64,
    audio_manager: &mut AudioManager,
    score: &mut ScoreManager,
) {
    if is_paused(data) || is_game_over(data) {
        return;
    }

    update_challenge_tps(data, dt);
    check_and_update_music_for_row(data, data.active_row_index);

    let scroll_speed = get_scroll_speed(data);
    let scroll_delta = scroll_speed * dt as f32;
    data.scroll_offset += scroll_delta;

    if data.has_game_started && data.midi_playing {
        let current_music_tps = data
            .musics_metadata
            .get(data.current_music_index)
            .map(|m| m.tps)
            .unwrap_or(DEFAULT_TPS);
        let speed_multiplier = data.current_tps / current_music_tps;

        data.current_midi_time += dt * speed_multiplier as f64;

        if data.current_midi_time >= data.target_time_for_next_note {
            data.current_midi_time = data.target_time_for_next_note - 0.0001;
            data.midi_playing = false;
        }

        if data.midi_loaded {
            let played_ids = audio_manager
                .update_midi_playback(data.current_midi_time, &data.skipped_midi_notes);
            spawn_note_hit_animations(data, &played_ids, current_time);
        }
    }

    // Update holding long tiles
    let active_idx = data.active_row_index;
    if let Some(row) = data.rows.get_mut(active_idx) {
        let mut any_completed = false;
        let row_clone = row.clone();
        for tile in &mut row.tiles {
            if tile.is_holding && !tile.is_pressed {
                tile.progress += scroll_delta;
                if tile.progress >= tile.height {
                    tile.progress = tile.height;
                    tile.is_holding = false;
                    tile.is_pressed = true;
                    tile.completed_at = Some(current_time);
                    score.add_tile_score(tile, &row_clone, current_time);
                    any_completed = true;
                }
            }
        }
        if any_completed && row.tiles.iter().all(|t| t.is_pressed && !t.is_holding) {
            row.is_completed = true;
            data.completed_rows_count += 1;
        }
    }

    update_active_row(data, audio_manager, current_time);
    check_and_handle_endless_loop(data, audio_manager);
}

pub fn update_game_over_flash(data: &mut GameData, current_time: f64) {
    if let Some(ref mut flash) = data.game_over_data {
        if !flash.is_flashing {
            return;
        }

        let elapsed = current_time - flash.start_time;
        const FLASH_INTERVAL: f64 = 125.0;
        const TOTAL_DURATION: f64 = 1000.0;

        if elapsed >= TOTAL_DURATION {
            flash.is_flashing = false;
            flash.tile.flash_state = false;
            return;
        }

        let flash_count = (elapsed / FLASH_INTERVAL) as u32;
        flash.flash_count = flash_count;
        flash.tile.flash_state = flash_count.is_multiple_of(2);
    }
}

pub fn update_game_over_animation(data: &mut GameData, current_time: f64) {
    if let Some(ref mut anim) = data.game_over_animation {
        if !anim.is_animating {
            return;
        }

        let elapsed = current_time - anim.start_time;
        let progress = (elapsed / anim.duration).min(1.0);

        // Cubic out easing
        let eased_progress = 1.0 - (1.0 - progress).powi(3);

        let new_offset =
            anim.start_offset + (anim.target_offset - anim.start_offset) * eased_progress as f32;
        data.scroll_offset = new_offset;

        if progress >= 1.0 {
            anim.is_animating = false;
        }
    }
}

pub fn update_game_won(data: &mut GameData, current_time: f64) {
    if data.state == GameState::Cleared
        && let Some(won_time) = data.game_won_time
        && current_time - won_time >= 1000.0
    {
        log::debug!("Game won! (TODO: reset game data)");
    }
}

fn check_and_update_music_for_row(data: &mut GameData, active_idx: usize) {
    let row = match data.rows.get(active_idx) {
        Some(r) => r,
        None => return,
    };
    if row.row_type == RowType::StartingTileRow {
        return;
    }

    let level_row_index = row.row_index.saturating_sub(1);
    let mut new_music_idx = None;
    for (i, music) in data.musics_metadata.iter().enumerate() {
        if level_row_index >= music.start_row_index && level_row_index < music.end_row_index {
            if data.current_music_index != i {
                new_music_idx = Some(i);
            }
            break;
        }
    }

    if let Some(i) = new_music_idx {
        data.current_music_index = i;
        if data.game_mode != GameMode::Survival {
            data.current_tps = data.musics_metadata[i].tps;
        }
    }
}

pub fn update_bot(
    data: &mut GameData,
    audio_manager: &mut AudioManager,
    score: &mut ScoreManager,
    current_time: f64,
) {
    if is_game_over(data) {
        return;
    }

    let active_idx = data.active_row_index;
    if data.rows[active_idx].row_type == RowType::StartingTileRow {
        return;
    }

    // 2. Determine trigger and type
    let scroll_offset = data.scroll_offset;
    let midi_loaded = data.midi_loaded;
    let (row_top, row_bottom, is_long_tile) = {
        let row = &data.rows[active_idx];
        let row_top = row.y_position + scroll_offset;
        let row_bottom = row_top + row.height;
        let is_long_tile = row.height > BASE_ROW_HEIGHT;
        (row_top, row_bottom, is_long_tile)
    };

    let trigger_y = SCREEN_HEIGHT / 2.0;

    if is_long_tile {
        let long_tile_trigger = row_bottom - BASE_ROW_HEIGHT;
        if long_tile_trigger >= trigger_y {
            let row_clone = data.rows[active_idx].clone();
            let mut new_interactions = 0;
            let mut ongoing_holds = 0;

            for tile in &mut data.rows[active_idx].tiles {
                if !tile.is_pressed && !tile.is_holding {
                    tile.is_holding = true;
                    tile.last_note_played_at = Some(current_time);
                    tile.active_circle_animations.push(current_time);
                    tile.progress = BASE_ROW_HEIGHT;
                    new_interactions += 1;
                } else if tile.is_holding {
                    ongoing_holds += 1;
                }
            }

            if new_interactions > 0 {
                if !data.has_game_started {
                    data.has_game_started = true;
                    data.current_midi_time = 0.0;
                }
                for _ in 0..new_interactions {
                    update_midi_playback_for_row(data, &row_clone, audio_manager, current_time);
                    play_tile_sound(midi_loaded, audio_manager);
                }
            }
            for _ in 0..ongoing_holds {
                update_midi_playback_for_row(data, &row_clone, audio_manager, current_time);
            }
        }
    } else {
        if row_top >= trigger_y {
            let row_clone = data.rows[active_idx].clone();
            let mut hits = 0;
            for tile in &mut data.rows[active_idx].tiles {
                if !tile.is_pressed {
                    tile.is_pressed = true;
                    tile.active_circle_animations.push(current_time);
                    tile.completed_at = Some(current_time);
                    hits += 1;
                }
            }

            if hits > 0 {
                if !data.has_game_started {
                    data.has_game_started = true;
                    data.current_midi_time = 0.0;
                }
                for _ in 0..hits {
                    update_midi_playback_for_row(data, &row_clone, audio_manager, current_time);
                    play_tile_sound(midi_loaded, audio_manager);
                }

                let row = &mut data.rows[active_idx];
                for tile in &mut row.tiles {
                    score.add_tile_score(tile, &row_clone, current_time);
                }

                if row.tiles.iter().all(|t| t.is_pressed) {
                    row.is_completed = true;
                    data.completed_rows_count += 1;
                    update_active_row(data, audio_manager, current_time);
                }
            }
        }
    }
}

pub fn update_midi_playback_for_row(
    data: &mut GameData,
    row: &RowData,
    audio_manager: &mut AudioManager,
    current_time: f64,
) {
    let level_row_index = row.row_index.saturating_sub(1);
    let timing = match data.level_row_timings.get(level_row_index) {
        Some(t) => t,
        None => return,
    };

    let is_manual = row.row_type != RowType::EmptyRow;
    let is_first = row.row_type != RowType::DoubleTileRow || data.current_dt_press_count == 0;

    if is_manual && is_first && data.current_midi_time < timing.start_time {
        log::debug!(
            "Jumping stopwatch to next timing point: {:.3}s -> {:.3}s",
            data.current_midi_time,
            timing.start_time
        );
        data.current_midi_time = timing.start_time;
    }

    data.current_dt_press_count += 1;
    if row.row_type == RowType::DoubleTileRow {
        if data.current_dt_press_count == 1 {
            data.target_time_for_next_note = timing.mid_time;
        } else {
            data.target_time_for_next_note = timing.end_time;
        }
    } else {
        data.target_time_for_next_note = timing.end_time;
    }

    data.midi_playing = true;

    if data.midi_loaded {
        let played_ids =
            audio_manager.update_midi_playback(data.current_midi_time, &data.skipped_midi_notes);
        spawn_note_hit_animations(data, &played_ids, current_time);
    }
}

pub fn spawn_note_hit_animations(data: &mut GameData, played_ids: &[i64], current_time: f64) {
    if played_ids.is_empty() {
        return;
    }

    let mut processed_hits = std::collections::HashSet::new();

    for &id in played_ids {
        if let Some(ind) = data.note_indicators.iter_mut().find(|i| i.note_id == id) {
            ind.is_consumed = true;
            let hit_key = format!("{}_{}", ind.row_index, ind.time);
            if !processed_hits.contains(&hit_key) {
                processed_hits.insert(hit_key);
                if let Some(row) = data.rows.get_mut(ind.row_index) {
                    for tile in &mut row.tiles {
                        if tile.is_holding {
                            tile.last_note_played_at = Some(current_time);
                            tile.active_circle_animations.push(current_time);
                        }
                    }
                }
            }
        }
    }
}

fn get_scroll_speed(data: &GameData) -> f32 {
    data.current_tps * BASE_ROW_HEIGHT
}

fn update_challenge_tps(data: &mut GameData, dt: f64) {
    if data.game_mode == GameMode::Survival
        && let Some(ref cfg) = data.endless_config
        && let Some(acc) = cfg.acceleration_rate
    {
        data.current_tps += acc * dt as f32;
    }
}

fn is_paused(data: &GameData) -> bool {
    matches!(
        data.state,
        GameState::Paused
            | GameState::TileMisclicked
            | GameState::TileFellOffScreen
            | GameState::Cleared
    )
}

fn is_game_over(data: &GameData) -> bool {
    matches!(
        data.state,
        GameState::Flashing
            | GameState::TileMisclicked
            | GameState::TileFellOffScreen
            | GameState::Cleared
    )
}

fn play_tile_sound(midi_loaded: bool, audio_manager: &AudioManager) {
    if !midi_loaded {
        audio_manager.play_random_sample();
    }
}

fn update_active_row(data: &mut GameData, audio_manager: &mut AudioManager, current_time: f64) {
    let active_idx = data.active_row_index;
    if let Some(row) = data.rows.get(active_idx)
        && row.row_type != RowType::StartingTileRow
    {
        let screen_y = row.y_position + data.scroll_offset;
        if screen_y > SCREEN_HEIGHT && !row.is_completed {
            trigger_game_over_out_of_bounds(data, current_time, audio_manager);
            return;
        }
    }

    let start_idx = active_idx.saturating_sub(5);
    let mut visible_incomplete_rows = Vec::new();

    for i in start_idx..data.rows.len() {
        let row = &data.rows[i];
        let screen_y = row.y_position + data.scroll_offset;
        let row_bottom = screen_y + row.height;

        if !row.is_completed && screen_y + row.height > 0.0 && screen_y < SCREEN_HEIGHT {
            visible_incomplete_rows.push(i);
        }

        if row_bottom < 0.0 {
            break;
        }
    }

    if !visible_incomplete_rows.is_empty() {
        // Find the lowest (highest y_position) row among visible incomplete ones
        let mut lowest_idx = visible_incomplete_rows[0];
        let mut max_y = data.rows[lowest_idx].y_position;
        for &idx in &visible_incomplete_rows {
            if data.rows[idx].y_position > max_y {
                max_y = data.rows[idx].y_position;
                lowest_idx = idx;
            }
        }

        if lowest_idx != active_idx {
            // Auto-complete empty rows skipped and update MIDI
            for i in active_idx..lowest_idx {
                if data.rows[i].row_type == RowType::EmptyRow {
                    let row_clone = data.rows[i].clone();
                    update_midi_playback_for_row(data, &row_clone, audio_manager, current_time);
                }
            }
            data.active_row_index = lowest_idx;
            data.current_dt_press_count = 0;
        }
    } else if data.game_mode == GameMode::OneRound {
        // Check for game won
        let has_incomplete = data.rows.iter().skip(active_idx).any(|r| !r.is_completed);
        if !has_incomplete
            && !data.rows.is_empty()
            && let Some(last_row) = data.rows.last()
        {
            let last_row_screen_y = last_row.y_position + data.scroll_offset;
            if last_row_screen_y > SCREEN_HEIGHT {
                trigger_game_won(data, current_time);
            }
        }
    }
}

fn trigger_game_won(data: &mut GameData, current_time: f64) {
    if data.state != GameState::Cleared {
        data.state = GameState::Cleared;
        data.game_won_time = Some(current_time);
    }
}

fn skip_notes_for_active_row(data: &mut GameData) {
    let active_idx = data.active_row_index;
    if let Some(row) = data.rows.get(active_idx) {
        let row_idx = row.row_index;
        let mut to_skip = Vec::new();
        for ind in &mut data.note_indicators {
            if ind.row_index == row_idx && !ind.is_consumed {
                ind.is_consumed = true;
                to_skip.push(ind.note_id);
            }
        }
        data.skipped_midi_notes.extend(to_skip);
    }
}

pub fn calculate_level_row_timings(
    rows: &[RowData],
    musics_metadata: &[MusicMetadata],
) -> Vec<RowTiming> {
    let mut timings = Vec::new();
    let mut cumulative_time = 0.0_f64;

    for row in rows.iter().skip(1) {
        let level_row_index = row.row_index.saturating_sub(1);
        let mut tps = DEFAULT_TPS as f64;

        for music in musics_metadata {
            if level_row_index >= music.start_row_index && level_row_index < music.end_row_index {
                tps = music.tps as f64;
                break;
            }
        }

        let row_start = cumulative_time;
        let row_duration = row.height_multiplier as f64 * (1.0 / tps);
        let row_end = cumulative_time + row_duration;

        timings.push(RowTiming {
            start_time: row_start,
            mid_time: (row_start + row_end) / 2.0,
            end_time: row_end,
        });

        cumulative_time = row_end;
    }
    timings
}

fn check_and_handle_endless_loop(data: &mut GameData, audio_manager: &mut AudioManager) {
    if data.game_mode == GameMode::OneRound {
        return;
    }
    if data.raw_level_rows.is_empty() {
        return;
    }

    if let Some(last_music) = data.musics_metadata.last().cloned()
        && data.current_music_index == data.musics_metadata.len() - 1
    {
        let rows_per_loop = last_music.end_row_index;
        let expected_total_rows = (data.loop_count as usize + 2) * rows_per_loop + 1;

        if data.rows.len() < expected_total_rows {
            append_level_loop(data, audio_manager);
            let cleanup_threshold = data.active_row_index.saturating_sub(100);
            data.note_indicators
                .retain(|ind| ind.row_index >= cleanup_threshold);
        }
    }
}

fn append_level_loop(data: &mut GameData, audio_manager: &mut AudioManager) {
    let raw_rows = data.raw_level_rows.clone();
    if raw_rows.is_empty() {
        return;
    }
    if data.rows.is_empty() {
        return;
    }

    let base_row_index = data.rows.len();
    let mut current_y = data.rows.last().unwrap().y_position;
    let mut last_single_lane = data
        .rows
        .last()
        .unwrap()
        .tiles
        .first()
        .map(|t| t.lane_index)
        .unwrap_or(0);

    let mut new_rows = Vec::new();

    for (i, row_data) in raw_rows.iter().enumerate() {
        let row_height = row_data.height_multiplier * BASE_ROW_HEIGHT;
        current_y -= row_height;
        let row_index = base_row_index + i;
        let preceding_row = new_rows.last().or(data.rows.last());
        let mut tiles = Vec::new();

        use rand::Rng;
        let mut rng = rand::thread_rng();

        if row_data.row_type == RowType::SingleTileRow {
            let lane = if let Some(pr) = preceding_row {
                if pr.row_type == RowType::DoubleTileRow {
                    let occupied: Vec<_> = pr.tiles.iter().map(|t| t.lane_index).collect();
                    let empty: Vec<_> = (0..4).filter(|l| !occupied.contains(l)).collect();
                    if empty.is_empty() {
                        0
                    } else {
                        empty[rng.gen_range(0..empty.len())]
                    }
                } else {
                    let avail: Vec<_> = (0..4).filter(|&l| l != last_single_lane).collect();
                    if avail.is_empty() {
                        0
                    } else {
                        avail[rng.gen_range(0..avail.len())]
                    }
                }
            } else {
                0
            };

            tiles.push(row_generator::create_tile(
                lane,
                current_y,
                row_height,
                GameColor::BLACK,
                1.0,
            ));
            last_single_lane = lane;
        } else if row_data.row_type == RowType::DoubleTileRow {
            let (l1, l2) = determine_double_lanes(preceding_row);
            tiles.push(row_generator::create_tile(
                l1,
                current_y,
                row_height,
                GameColor::BLACK,
                1.0,
            ));
            tiles.push(row_generator::create_tile(
                l2,
                current_y,
                row_height,
                GameColor::BLACK,
                1.0,
            ));
        }

        new_rows.push(RowData {
            row_index,
            row_type: row_data.row_type,
            height_multiplier: row_data.height_multiplier,
            y_position: current_y,
            height: row_height,
            tiles,
            is_completed: row_data.row_type == RowType::EmptyRow,
            is_active: false,
        });
    }

    data.rows.extend(new_rows);

    let original_total = raw_rows.len();
    let original_musics: Vec<_> = data
        .musics_metadata
        .iter()
        .filter(|m| m.start_row_index < original_total)
        .cloned()
        .collect();
    let new_loop_offset = (data.loop_count as usize + 1) * original_total;

    let mut tps_acc = data
        .musics_metadata
        .iter()
        .rfind(|m| m.start_row_index < original_total)
        .map(|m| m.tps)
        .unwrap_or(DEFAULT_TPS);
    for m in &original_musics {
        if data.game_mode == GameMode::Endless {
            tps_acc += 0.333;
        }
        data.musics_metadata.push(MusicMetadata {
            id: m.id,
            tps: if data.game_mode == GameMode::Endless {
                tps_acc
            } else {
                m.tps
            },
            bpm: m.bpm,
            base_beats: m.base_beats,
            start_row_index: m.start_row_index + new_loop_offset,
            end_row_index: m.end_row_index + new_loop_offset,
            row_count: m.row_count,
        });
    }

    let new_timings = calculate_level_row_timings(&data.rows, &data.musics_metadata);
    data.level_row_timings = new_timings.clone();

    let lp0_indicators: Vec<_> = data
        .note_indicators
        .iter()
        .filter(|i| i.row_index <= original_total)
        .cloned()
        .collect();
    let mut new_indicators = Vec::new();

    for ind in lp0_indicators {
        if let (Some(tf), Some(trk), Some(mid)) = (ind.time_fraction, ind.track_idx, ind.midi) {
            let new_row_idx = ind.row_index + new_loop_offset;
            if let Some(nr_timing) = new_timings.get(new_row_idx.saturating_sub(1)) {
                let new_time =
                    nr_timing.start_time + tf * (nr_timing.end_time - nr_timing.start_time);

                if let Some(new_row) = data.rows.get(new_row_idx) {
                    let row_bottom = new_row.y_position + new_row.height;
                    let base_edge = row_bottom - BASE_ROW_HEIGHT;
                    let ind_y = base_edge - tf as f32 * new_row.height - 8.0;

                    let new_id = (new_time * 1000.0).round() as i64 * 1000000
                        + trk as i64 * 1000
                        + mid as i64;

                    new_indicators.push(NoteIndicatorData {
                        note_id: new_id,
                        row_index: new_row_idx,
                        x: ind.x,
                        y: ind_y,
                        width: ind.width,
                        height: ind.height,
                        time: new_time,
                        time_fraction: Some(tf),
                        track_idx: Some(trk),
                        midi: Some(mid),
                        is_consumed: false,
                    });
                }
            }
        }
    }

    // Playback loop notes (includes non-indicator invisible background tracks)
    let loop_0_notes = data.loop_0_midi_notes.clone();
    for mn in loop_0_notes {
        let new_row_idx = mn.row_index + new_loop_offset;
        if let Some(nr_timing) = new_timings.get(new_row_idx.saturating_sub(1)) {
            let new_time = nr_timing.start_time
                + mn.time_fraction * (nr_timing.end_time - nr_timing.start_time);
            audio_manager.add_dynamic_midi_note(mn.track_idx, mn.midi, new_time, mn.duration);
        }
    }

    // Clear active note tracking from the previous loop so that stale note_off
    // events (scheduled at end_times from the old loop) don't kill notes that
    // are re-triggered in the new loop with the same MIDI number.
    audio_manager.clear_active_notes();

    data.note_indicators.extend(new_indicators);
    data.loop_count += 1;
}

fn determine_double_lanes(preceding_row: Option<&RowData>) -> (u32, u32) {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    if let Some(row) = preceding_row {
        match row.row_type {
            RowType::SingleTileRow | RowType::StartingTileRow => {
                if let Some(t) = row.tiles.first() {
                    if t.lane_index == 0 || t.lane_index == 2 {
                        return (1, 3);
                    } else {
                        return (0, 2);
                    }
                }
            }
            RowType::DoubleTileRow => {
                let occupied: Vec<_> = row.tiles.iter().map(|t| t.lane_index).collect();
                if occupied.contains(&0) && occupied.contains(&2) {
                    return (1, 3);
                } else {
                    return (0, 2);
                }
            }
            _ => {}
        }
    }
    if rng.gen_bool(0.5) { (0, 2) } else { (1, 3) }
}

pub fn handle_tile_press(
    data: &mut GameData,
    lane: u32,
    y_coord: Option<f32>,
    input_type: InputType,
    current_time: f64,
    audio_manager: &mut AudioManager,
    score: &mut ScoreManager,
    enable_autoplay: bool,
) -> bool {
    if is_game_over(data) || data.state == GameState::Cleared {
        return false;
    }

    let active_idx = data.active_row_index;
    if active_idx >= data.rows.len() {
        return false;
    }

    // 1. Starting tile logic (can happen in Paused state)
    if data.state == GameState::Paused {
        let row = &data.rows[active_idx];
        if row.row_type == RowType::StartingTileRow
            && let Some(tile) = row.tiles.first()
            && tile.lane_index == lane
        {
            if let Some(y) = y_coord {
                let screen_y = tile.y + data.scroll_offset;
                if y < screen_y || y > screen_y + tile.height {
                    return false;
                }
            }
            // Success!
            let start_tile = &mut data.rows[active_idx].tiles[0];
            start_tile.is_pressed = true;
            start_tile.completed_at = Some(current_time);

            data.rows[active_idx].is_completed = true;
            data.state = GameState::Resumed;
            data.completed_rows_count += 1;
            data.current_dt_press_count = 0;
            update_active_row(data, audio_manager, current_time);
            return true;
        }
        return false;
    }

    if data.state != GameState::Resumed {
        return false;
    }

    let midi_loaded = data.midi_loaded;
    let row_bottom = {
        let row = &data.rows[active_idx];
        row.y_position + data.scroll_offset + row.height
    };
    let row_top = row_bottom - data.rows[active_idx].height;

    // 2. Input filtering
    let mut is_in_valid_y = false;
    if input_type == InputType::Keyboard {
        let timing_zone = SCREEN_HEIGHT / 2.0;
        if row_bottom >= timing_zone {
            is_in_valid_y = true;
        }
    } else {
        if let Some(y) = y_coord
            && y >= row_top
            && y <= row_bottom
        {
            is_in_valid_y = true;
        }
    }

    if !is_in_valid_y {
        return false;
    }

    if enable_autoplay {
        let has_tile = data.rows[active_idx]
            .tiles
            .iter()
            .any(|t| t.lane_index == lane);
        if !has_tile {
            trigger_game_over_misclicked(data, lane, current_time, audio_manager);
        }
        return false;
    }

    let tile_idx = data.rows[active_idx]
        .tiles
        .iter()
        .position(|t| t.lane_index == lane && !t.is_pressed && !t.is_holding);

    match tile_idx {
        Some(idx) => {
            let row_clone = data.rows[active_idx].clone();
            let is_long = row_clone.height_multiplier > 1.0;

            if is_long
                && input_type == InputType::Mouse
                && let Some(y) = y_coord
            {
                let hit_zone_top = row_bottom - BASE_ROW_HEIGHT;
                if y < hit_zone_top || y > row_bottom {
                    return true; // Match TS: consumed but do nothing
                }
            }

            check_and_update_music_for_row(data, active_idx);

            if !data.has_game_started {
                data.has_game_started = true;
                data.current_midi_time = 0.0;
            }

            update_midi_playback_for_row(data, &row_clone, audio_manager, current_time);

            if !data.rows[active_idx].is_completed {
                play_tile_sound(midi_loaded, audio_manager);
            }

            let mut needs_active_update = false;
            {
                let row = &mut data.rows[active_idx];
                let tile = &mut row.tiles[idx];

                if is_long {
                    tile.is_holding = true;
                    tile.last_note_played_at = Some(current_time);
                    tile.progress = BASE_ROW_HEIGHT;
                } else {
                    tile.is_pressed = true;
                    tile.completed_at = Some(current_time);
                    score.add_tile_score(tile, &row_clone, current_time);
                }

                tile.active_circle_animations.push(current_time);

                if !is_long {
                    let all_pressed = row.tiles.iter().all(|t| t.is_pressed);
                    if all_pressed {
                        row.is_completed = true;
                        data.completed_rows_count += 1;
                        needs_active_update = true;
                    }
                }
            }
            if needs_active_update {
                update_active_row(data, audio_manager, current_time);
            }
            true
        }
        None => {
            // Misclick if lane is wrong AND we are in valid Y
            let row = &data.rows[active_idx];
            if !row.tiles.iter().any(|t| t.lane_index == lane) {
                trigger_game_over_misclicked(data, lane, current_time, audio_manager);
            }
            false
        }
    }
}

pub fn handle_tile_release(
    data: &mut GameData,
    lane: u32,
    current_time: f64,
    _audio_manager: &mut AudioManager,
    score: &mut ScoreManager,
    enable_autoplay: bool,
) {
    if enable_autoplay {
        return;
    }
    if data.state != GameState::Resumed || !data.has_game_started {
        return;
    }

    let active_idx = data.active_row_index;
    if active_idx >= data.rows.len() {
        return;
    }

    let row = &mut data.rows[active_idx];
    if row.height_multiplier <= 1.0 {
        return;
    }

    let mut score_tiles: Vec<(TileData, RowData)> = Vec::new();
    let row_clone = row.clone();
    let mut skipped = false;
    for tile in &mut row.tiles {
        if tile.lane_index == lane && tile.is_holding {
            tile.is_holding = false;
            tile.is_pressed = true; // IMPORTANT
            if tile.progress < tile.height {
                tile.is_released_early = true;
                skipped = true;
            }
            tile.completed_at = Some(current_time);
            score_tiles.push((tile.clone(), row_clone.clone()));
        }
    }

    if skipped {
        skip_notes_for_active_row(data);
    }

    for (tile_clone, row_clone) in &score_tiles {
        score.add_tile_score(tile_clone, row_clone, current_time);
    }

    let row = &data.rows[active_idx];
    let all_released = row.tiles.iter().all(|t| t.is_pressed && !t.is_holding);
    if all_released {
        data.rows[active_idx].is_completed = true;
        data.completed_rows_count += 1;
        update_active_row(data, _audio_manager, current_time);
    }
}

pub fn trigger_game_over_misclicked(
    data: &mut GameData,
    lane: u32,
    current_time: f64,
    audio_manager: &mut AudioManager,
) {
    if is_game_over(data) {
        return;
    }
    let active_idx = data.active_row_index;
    if active_idx >= data.rows.len() {
        return;
    }
    let row = &data.rows[active_idx];
    let tile = TileData {
        lane_index: lane,
        x: row_generator::calculate_lane_x(lane),
        y: row.y_position,
        width: COLUMN_WIDTH,
        height: row.height,
        color: GameColor::RED,
        opacity: 1.0,
        is_pressed: false,
        is_game_over_indicator: true,
        flash_state: true,
        is_holding: false,
        progress: 0.0,
        is_released_early: false,
        completed_at: None,
        last_note_played_at: None,
        active_circle_animations: Vec::new(),
    };
    data.game_over_data = Some(GameOverFlashState {
        tile,
        start_time: current_time,
        flash_count: 0,
        is_flashing: true,
    });
    data.state = GameState::TileMisclicked;
    audio_manager.play_game_over_chord();
}

pub fn trigger_game_over_out_of_bounds(
    data: &mut GameData,
    current_time: f64,
    audio_manager: &mut AudioManager,
) {
    if is_game_over(data) {
        return;
    }
    data.state = GameState::TileFellOffScreen;

    let active_idx = data.active_row_index;
    if let Some(row) = data.rows.get(active_idx)
        && let Some(unpressed_tile) = row.tiles.iter().find(|t| !t.is_pressed)
    {
        data.game_over_data = Some(GameOverFlashState {
            tile: unpressed_tile.clone(),
            start_time: current_time,
            flash_count: 0,
            is_flashing: true,
        });
    }

    let target_offset = calculate_reposition_offset(data);
    data.game_over_animation = Some(GameOverAnimationState {
        start_time: current_time,
        duration: 500.0,
        start_offset: data.scroll_offset,
        target_offset,
        is_animating: true,
    });
    audio_manager.play_game_over_chord();
}

fn calculate_reposition_offset(data: &GameData) -> f32 {
    let active_idx = data.active_row_index;
    if let Some(row) = data.rows.get(active_idx) {
        SCREEN_HEIGHT - BASE_ROW_HEIGHT - row.height - row.y_position
    } else {
        data.scroll_offset
    }
}

pub fn create_game_data(
    rows: Vec<RowData>,
    musics_metadata: Vec<MusicMetadata>,
    game_mode: GameMode,
    endless_config: Option<EndlessConfig>,
    raw_level_rows: Vec<RowTypeResult>,
    filename: String,
) -> GameData {
    let initial_tps = if !musics_metadata.is_empty() {
        musics_metadata[0].tps
    } else {
        DEFAULT_TPS
    };

    let current_tps = if game_mode == GameMode::Survival {
        endless_config
            .as_ref()
            .and_then(|c| c.starting_tps)
            .unwrap_or(initial_tps)
    } else {
        initial_tps
    };

    log::info!("Loading level:");
    log::info!("  - Initial TPS: {:.2}", current_tps);

    let mut data = GameData {
        state: GameState::Paused,
        rows,
        total_completed_height: 0.0,
        scroll_offset: 0.0,
        game_over_data: None,
        game_over_animation: None,
        game_won_time: None,
        last_single_lane: 0,
        last_double_lanes: None,
        active_row_index: 0,
        completed_rows_count: 0,
        current_tps,
        current_music_index: 0,
        musics_metadata,
        current_midi_time: 0.0,
        midi_loaded: false,
        has_game_started: false,
        note_indicators: Vec::new(),
        midi_playing: false,
        target_time_for_next_note: 0.0,
        current_dt_press_count: 0,
        skipped_midi_notes: Vec::new(),
        level_row_timings: Vec::new(),
        game_mode,
        endless_config,
        loop_count: 0,
        current_filename: filename,
        raw_level_rows,
        loop_0_midi_notes: Vec::new(),
    };
    let timings = calculate_level_row_timings(&data.rows, &data.musics_metadata);
    data.level_row_timings = timings;
    data
}
