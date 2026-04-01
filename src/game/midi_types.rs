use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// MIDI note number to note-name mapping.
pub fn midi_to_note(midi: u8) -> Option<&'static str> {
    MIDI_TO_NOTE.get(&midi).copied()
}

/// Note-name to MIDI number mapping.
pub fn note_to_midi(name: &str) -> Option<u8> {
    NOTE_TO_MIDI.get(name).copied()
}

// We use phf or just static arrays. For simplicity, build HashMaps at init time.
use std::sync::LazyLock;

static NOTE_TO_MIDI: LazyLock<HashMap<&'static str, u8>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    let entries: &[(&str, u8)] = &[
        ("c5", 108),
        ("b4", 107),
        ("#a4", 106),
        ("a4", 105),
        ("#g4", 104),
        ("g4", 103),
        ("#f4", 102),
        ("f4", 101),
        ("e4", 100),
        ("#d4", 99),
        ("d4", 98),
        ("#c4", 97),
        ("c4", 96),
        ("b3", 95),
        ("#a3", 94),
        ("a3", 93),
        ("#g3", 92),
        ("g3", 91),
        ("#f3", 90),
        ("f3", 89),
        ("e3", 88),
        ("#d3", 87),
        ("d3", 86),
        ("#c3", 85),
        ("c3", 84),
        ("b2", 83),
        ("#a2", 82),
        ("a2", 81),
        ("#g2", 80),
        ("g2", 79),
        ("#f2", 78),
        ("f2", 77),
        ("e2", 76),
        ("#d2", 75),
        ("d2", 74),
        ("#c2", 73),
        ("c2", 72),
        ("b1", 71),
        ("#a1", 70),
        ("a1", 69),
        ("#g1", 68),
        ("g1", 67),
        ("#f1", 66),
        ("f1", 65),
        ("e1", 64),
        ("#d1", 63),
        ("d1", 62),
        ("#c1", 61),
        ("c1", 60),
        ("b", 59),
        ("#a", 58),
        ("a", 57),
        ("#g", 56),
        ("g", 55),
        ("#f", 54),
        ("f", 53),
        ("e", 52),
        ("#d", 51),
        ("d", 50),
        ("#c", 49),
        ("c", 48),
        ("B-1", 47),
        ("#A-1", 46),
        ("A-1", 45),
        ("#G-1", 44),
        ("G-1", 43),
        ("#F-1", 42),
        ("F-1", 41),
        ("E-1", 40),
        ("#D-1", 39),
        ("D-1", 38),
        ("#C-1", 37),
        ("C-1", 36),
        ("B-2", 35),
        ("#A-2", 34),
        ("A-2", 33),
        ("#G-2", 32),
        ("G-2", 31),
        ("#F-2", 30),
        ("F-2", 29),
        ("E-2", 28),
        ("#D-2", 27),
        ("D-2", 26),
        ("#C-2", 25),
        ("C-2", 24),
        ("B-3", 23),
        ("#A-3", 22),
        ("A-3", 21),
        ("mute", 1),
        ("empty", 1),
    ];
    for &(name, midi) in entries {
        m.insert(name, midi);
    }
    m
});

static MIDI_TO_NOTE: LazyLock<HashMap<u8, &'static str>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    for (&name, &midi) in NOTE_TO_MIDI.iter() {
        if (21..=108).contains(&midi) {
            m.entry(midi).or_insert(name);
        }
    }
    m
});

pub static BASE_BEATS_MAP: LazyLock<HashMap<&'static str, f64>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    let entries: &[(&str, f64)] = &[
        ("15", 1.0),
        ("7.5", 2.0),
        ("5", 3.0),
        ("3.75", 4.0),
        ("3", 5.0),
        ("2.5", 6.0),
        ("1.875", 8.0),
        ("1.5", 10.0),
        ("1.25", 12.0),
        ("1", 15.0),
        ("0.9375", 16.0),
        ("0.75", 20.0),
        ("0.625", 24.0),
        ("0.5", 30.0),
        ("0.46875", 32.0),
        ("0.375", 40.0),
        ("0.3125", 48.0),
        ("0.25", 60.0),
        ("0.234375", 64.0),
        ("0.1875", 80.0),
        ("0.15625", 96.0),
        ("0.125", 120.0),
        ("0.1171875", 128.0),
        ("0.09375", 160.0),
        ("0.078125", 192.0),
        ("0.0625", 240.0),
        ("0.05859375", 256.0),
        ("0.046875", 320.0),
        ("0.0390625", 384.0),
        ("0.03125", 480.0),
        ("0.029296875", 512.0),
        ("0.0234375", 640.0),
        ("0.01953125", 768.0),
        ("0.015625", 960.0),
    ];
    for &(k, v) in entries {
        m.insert(k, v);
    }
    m
});

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidiHeader {
    pub ppq: u32,
    pub tempos: Vec<MidiTempo>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidiTempo {
    pub ticks: i64,
    pub bpm: f64,
    #[serde(default)]
    pub time: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidiNote {
    pub midi: u8,
    #[serde(default)]
    pub name: Option<String>,
    pub ticks: i64,
    pub time: f64,
    pub duration: f64,
    pub duration_ticks: i64,
    pub velocity: f64,
    pub note_off_velocity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidiTrack {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub channel: Option<u8>,
    pub notes: Vec<MidiNote>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidiJson {
    pub header: MidiHeader,
    pub tracks: Vec<MidiTrack>,
}

/// Internal parsed message types used during score parsing.
#[derive(Debug, Clone)]
pub struct ParsedMessage {
    /// 0=NoteOn, 1=NoteOff, 2=Wait, 3=Noop
    pub msg_type: u8,
    pub value: i64,
}

#[derive(Debug, Clone)]
pub struct ParsedTrack {
    pub base_beats: f64,
    pub messages: Vec<ParsedMessage>,
}

#[derive(Debug, Clone)]
pub struct ParsedPart {
    pub bpm: f64,
    pub base_beats: f64,
    pub tracks: Vec<ParsedTrack>,
}

/// Raw JSON music entry format, matching the JSON level file structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawMusicEntry {
    pub id: u32,
    #[serde(default)]
    pub bpm: Option<f64>,
    #[serde(rename = "baseBeats")]
    pub base_beats: f64,
    pub scores: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawMusicInputFile {
    #[serde(rename = "baseBpm")]
    pub base_bpm: f64,
    pub musics: Vec<RawMusicEntry>,
}
