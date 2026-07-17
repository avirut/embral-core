//! Post-recording speaker pipeline.
//!
//! Runs inside `finalize_meeting` before the transcript is formatted:
//! diarize the full recording → per-cluster voice embeddings → match against
//! the saved registry (Off / Suggest / Automatic) → write display labels and
//! registry links onto the transcript segments. Mic-channel dominance acts
//! as a silent prior: a strongly mic-dominant cluster that voice matching
//! can't place becomes a "sounds like you" suggestion (never an automatic
//! assignment). All decision math is pure and lives in
//! `embral_engine::speakers`; this module is the glue that owns audio
//! slicing, the registry lookups, and the labeling policy.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use embral_db::{Db, VoiceRefKind};
use embral_engine::speakers::{self as math, ChannelWindow, MatchMode, Outcome};
use embral_engine::{DiarizedSpan, Engine};
use embral_types::{AppConfig, SpeakerMatchMode, TranscriptionSegment};

/// One pending "Speaker N sounds like X" match, persisted per meeting until
/// confirmed or dismissed. The centroid rides along so confirmation can store
/// a learned voice reference without re-reading the audio.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SpeakerSuggestion {
    pub label: String,
    pub speaker_id: String,
    pub name: String,
    pub score: f32,
    pub centroid: Vec<f32>,
}

pub struct PipelineInput {
    pub samples: Arc<Vec<f32>>,
    pub channel_windows: Vec<ChannelWindow>,
    pub meeting_id: String,
}

/// Spans shorter than this are too thin for a reliable voice embedding.
const MIN_EMBED_SPAN_SECS: f64 = 1.5;
/// Enough voice per cluster; embedding more buys nothing.
const MAX_EMBED_SECS: f64 = 30.0;
/// Concatenated short turns must reach this length before we embed them.
const MIN_EMBED_TOTAL_SECS: f64 = 1.0;
const SAMPLE_RATE: f64 = 16_000.0;

/// Read a 16 kHz mono WAV (the recorder's own format, f32 or i16) back into
/// samples for diarization.
pub fn read_wav_16k(path: &Path) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)
        .with_context(|| format!("open {}", path.display()))?;
    let spec = reader.spec();
    anyhow::ensure!(
        spec.sample_rate == 16_000 && spec.channels == 1,
        "expected 16 kHz mono, got {} Hz / {} ch",
        spec.sample_rate,
        spec.channels
    );
    let samples = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<std::result::Result<Vec<_>, _>>()?,
        hound::SampleFormat::Int => reader
            .samples::<i16>()
            .map(|s| s.map(|v| v as f32 / i16::MAX as f32))
            .collect::<std::result::Result<Vec<_>, _>>()?,
    };
    Ok(samples)
}

/// Run the full pipeline, labeling `segments` in place (overwriting any
/// provisional live labels). Returns the pending suggestions to persist for
/// the meeting. CPU-heavy — call from a blocking context. Any error leaves
/// the segments as they came in (the meeting still finishes, keeping live
/// labels when they exist).
pub fn run(
    engine: &Engine,
    db: &Db,
    config: &AppConfig,
    input: &PipelineInput,
    segments: &mut [TranscriptionSegment],
) -> Result<Vec<SpeakerSuggestion>> {
    let started = std::time::Instant::now();
    let spans = engine.diarize(
        &input.samples,
        config.diarization_sensitivity.clustering_threshold(),
    )?;
    if spans.is_empty() {
        tracing::info!("diarization found no speech turns; leaving segments unlabeled");
        return Ok(Vec::new());
    }

    // Clusters in order of first appearance — that order drives numbering.
    let mut clusters: Vec<usize> = Vec::new();
    for s in &spans {
        if !clusters.contains(&s.cluster) {
            clusters.push(s.cluster);
        }
    }

    let centroids: HashMap<usize, Vec<f32>> = clusters
        .iter()
        .filter_map(|&c| cluster_centroid(engine, &input.samples, &spans, c).map(|e| (c, e)))
        .collect();

    // Map segments onto clusters up front: it drives the final label write
    // AND lets user-given live names (renamed pills during the recording)
    // claim the clusters their segments cover — an explicit name outranks
    // anything this pass could infer.
    let times: Vec<(f64, f64)> = segments.iter().map(|s| (s.start, s.end)).collect();
    let seg_clusters = math::label_segments(&times, &spans);
    let user_labels = dominant_user_labels(segments, &seg_clusters);
    let profile_id_by_name: HashMap<String, String> = if user_labels.is_empty() {
        HashMap::new()
    } else {
        db.list_speakers()?
            .into_iter()
            .map(|p| (p.name.to_lowercase(), p.id))
            .collect()
    };

    // --- Registry matching -------------------------------------------------
    let mode = match config.speaker_match_mode {
        SpeakerMatchMode::Off => MatchMode::Off,
        SpeakerMatchMode::Suggest => MatchMode::Suggest,
        SpeakerMatchMode::Automatic => MatchMode::Automatic,
    };
    // Newest-first per speaker (the store returns them in that order).
    let mut refs_by_speaker: HashMap<String, Vec<Vec<f32>>> = HashMap::new();
    if mode != MatchMode::Off {
        for r in db.all_voice_refs()? {
            refs_by_speaker.entry(r.speaker_id).or_default().push(r.embedding);
        }
    }

    // Silent "you" prior: needs a you-profile on file (onboarding or the
    // Profiles page creates it — nothing is created here), matching enabled,
    // and channel evidence to weigh.
    let you = if mode != MatchMode::Off && !input.channel_windows.is_empty() {
        db.you_speaker()?
    } else {
        None
    };

    let mut assignments: HashMap<usize, (String, Option<String>)> = HashMap::new();
    let mut suggestions: Vec<SpeakerSuggestion> = Vec::new();
    let mut next_number = 1usize;

    for &cluster in &clusters {
        // A user named this cluster live — keep the name (linked to its
        // profile when one matches), no numbered label, no suggestion.
        if let Some(name) = user_labels.get(&cluster) {
            let id = profile_id_by_name.get(&name.to_lowercase()).cloned();
            assignments.insert(cluster, (name.clone(), id));
            continue;
        }

        let centroid = centroids.get(&cluster);
        let best = centroid.and_then(|c| {
            refs_by_speaker
                .iter()
                .map(|(id, refs)| (id.as_str(), math::score(c, refs)))
                .max_by(|a, b| a.1.total_cmp(&b.1))
        });

        match math::decide(best, mode) {
            Outcome::Auto { speaker_id, learn } => {
                if let Some(person) = db.get_speaker(&speaker_id)? {
                    if learn {
                        if let Some(c) = centroid {
                            let _ = db.add_voice_ref(
                                &speaker_id,
                                VoiceRefKind::Learned,
                                None,
                                c,
                                None,
                                Some(&input.meeting_id),
                            );
                        }
                    }
                    assignments.insert(cluster, (person.name, Some(speaker_id)));
                    continue;
                }
                // Registry row vanished between scoring and lookup — fall
                // through to a numbered label.
            }
            Outcome::Suggest { speaker_id, score } => {
                if let Some(person) = db.get_speaker(&speaker_id)? {
                    let label = format!("Speaker {next_number}");
                    suggestions.push(SpeakerSuggestion {
                        label,
                        speaker_id,
                        name: person.name,
                        score,
                        centroid: centroid.cloned().unwrap_or_default(),
                    });
                }
            }
            Outcome::Unknown => {
                // Voice matching came up empty — offer the mic-dominance
                // prior instead. Confirming stores a learned voice reference
                // like any suggestion, seeding embedding-based matching.
                if let Some(you) = you.as_ref() {
                    let dominance =
                        cluster_mic_dominance(&input.channel_windows, &spans, cluster);
                    if dominance >= math::YOU_DOMINANCE {
                        suggestions.push(SpeakerSuggestion {
                            label: format!("Speaker {next_number}"),
                            speaker_id: you.id.clone(),
                            name: you.name.clone(),
                            score: dominance,
                            centroid: centroid.cloned().unwrap_or_default(),
                        });
                    }
                }
            }
        }

        assignments.insert(cluster, (format!("Speaker {next_number}"), None));
        next_number += 1;
    }

    // --- Label the transcript ----------------------------------------------
    // Wholesale: this pass is the authority, so any provisional live labels
    // the session produced are overwritten (or cleared where diarization
    // found no overlapping speech) — user-given names having been folded
    // into `assignments` above.
    for (seg, cluster) in segments.iter_mut().zip(seg_clusters.iter().copied()) {
        let assigned = cluster.and_then(|c| assignments.get(&c));
        seg.speaker = assigned.map(|(name, _)| name.clone());
        seg.speaker_id = assigned.and_then(|(_, id)| id.clone());
    }

    // Suggestions for clusters that ended up owning no segments are noise.
    let present: HashSet<&str> = segments
        .iter()
        .filter_map(|s| s.speaker.as_deref())
        .collect();
    suggestions.retain(|s| present.contains(s.label.as_str()));

    tracing::info!(
        clusters = clusters.len(),
        suggestions = suggestions.len(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "speaker pipeline finished"
    );
    Ok(suggestions)
}

/// A session-generated numbered label ("Speaker 3") as opposed to a name a
/// user typed over a pill.
fn is_generic_label(label: &str) -> bool {
    label
        .strip_prefix("Speaker ")
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

/// The user-given name for each diarized cluster, if any: among a cluster's
/// segments, the most frequent incoming non-generic label. Incoming labels
/// are the session's live labels after any pill renames — generic
/// "Speaker N" labels are machine guesses and carry no vote.
fn dominant_user_labels(
    segments: &[TranscriptionSegment],
    seg_clusters: &[Option<usize>],
) -> HashMap<usize, String> {
    let mut votes: HashMap<usize, HashMap<&str, usize>> = HashMap::new();
    for (seg, cluster) in segments.iter().zip(seg_clusters.iter().copied()) {
        if let (Some(cluster), Some(label)) = (cluster, seg.speaker.as_deref()) {
            if !is_generic_label(label) {
                *votes.entry(cluster).or_default().entry(label).or_default() += 1;
            }
        }
    }
    votes
        .into_iter()
        .filter_map(|(cluster, counts)| {
            counts
                .into_iter()
                .max_by_key(|(_, n)| *n)
                .map(|(label, _)| (cluster, label.to_string()))
        })
        .collect()
}

/// Voice embedding centroid for one cluster: embed each long-enough span
/// (up to a budget) and average; short-turn clusters fall back to one
/// embedding over their concatenated audio.
fn cluster_centroid(
    engine: &Engine,
    samples: &[f32],
    spans: &[DiarizedSpan],
    cluster: usize,
) -> Option<Vec<f32>> {
    let slice = |s: &DiarizedSpan| -> &[f32] {
        let a = ((s.start * SAMPLE_RATE) as usize).min(samples.len());
        let b = ((s.end * SAMPLE_RATE) as usize).min(samples.len());
        &samples[a..b]
    };

    let mut embeddings: Vec<Vec<f32>> = Vec::new();
    let mut used_secs = 0.0f64;
    for s in spans.iter().filter(|s| s.cluster == cluster) {
        if used_secs >= MAX_EMBED_SECS {
            break;
        }
        if s.end - s.start < MIN_EMBED_SPAN_SECS {
            continue;
        }
        if let Ok(e) = engine.embed(slice(s)) {
            embeddings.push(e);
            used_secs += s.end - s.start;
        }
    }

    if embeddings.is_empty() {
        // Only short turns — concatenate them and embed once.
        let mut concat: Vec<f32> = Vec::new();
        for s in spans.iter().filter(|s| s.cluster == cluster) {
            if concat.len() as f64 / SAMPLE_RATE >= MAX_EMBED_SECS {
                break;
            }
            concat.extend_from_slice(slice(s));
        }
        if (concat.len() as f64 / SAMPLE_RATE) < MIN_EMBED_TOTAL_SECS {
            return None;
        }
        embeddings.push(engine.embed(&concat).ok()?);
    }

    math::centroid(&embeddings)
}

/// Share of a cluster's speech time that was mic-dominant (each span's
/// fraction weighted by its length).
fn cluster_mic_dominance(
    windows: &[ChannelWindow],
    spans: &[DiarizedSpan],
    cluster: usize,
) -> f32 {
    let mut weighted = 0.0f64;
    let mut total = 0.0f64;
    for s in spans.iter().filter(|s| s.cluster == cluster) {
        let len = s.end - s.start;
        weighted += math::mic_dominant_fraction(windows, s.start, s.end) as f64 * len;
        total += len;
    }
    if total == 0.0 {
        0.0
    } else {
        (weighted / total) as f32
    }
}

/// Full-pipeline e2e over real weights: set `EMBRAL_TEST_DIARIZE_WAV` to a
/// two-speaker 16 kHz WAV (speaker A first) and download `speaker-id`, then
/// `cargo test -p embral --lib speaker_pipeline -- --ignored --nocapture`.
#[cfg(test)]
mod tests {
    use super::*;
    use embral_db::SpeakerRow;
    use embral_types::TranscriptionSegment;

    #[test]
    fn user_labels_outvote_generics_per_cluster() {
        let seg = |speaker: Option<&str>| TranscriptionSegment {
            speaker: speaker.map(String::from),
            speaker_id: None,
            text: "hi".into(),
            start: 0.0,
            end: 1.0,
        };
        let segments = vec![
            seg(Some("Speaker 1")), // machine guess — no vote
            seg(Some("Avirut")),
            seg(Some("Avirut")),
            seg(Some("Speaker 2")),
            seg(None),
        ];
        let clusters = vec![Some(0), Some(0), Some(0), Some(1), None];
        let labels = dominant_user_labels(&segments, &clusters);
        assert_eq!(labels.get(&0).map(String::as_str), Some("Avirut"));
        assert!(!labels.contains_key(&1), "generic labels carry no vote");

        assert!(is_generic_label("Speaker 12"));
        assert!(!is_generic_label("Speaker"));
        assert!(!is_generic_label("Speaker Twelve"));
        assert!(!is_generic_label("Sam"));
    }

    // The "sounds like you" prior fires on cluster_mic_dominance >=
    // math::YOU_DOMINANCE; the per-window fraction itself is tested in
    // embral-engine. This covers the span-length weighting on top of it.
    #[test]
    fn cluster_dominance_weights_spans_by_length() {
        let w = |start: f64, end: f64, mic: f32, lp: f32| ChannelWindow {
            start,
            end,
            mic_rms: mic,
            loop_rms: lp,
        };
        // Mic-dominant for the first 3 s, loopback-dominant after.
        let windows = vec![w(0.0, 3.0, 0.5, 0.01), w(3.0, 4.0, 0.01, 0.5)];
        let spans = vec![
            DiarizedSpan { start: 0.0, end: 3.0, cluster: 0 },
            DiarizedSpan { start: 3.0, end: 4.0, cluster: 0 },
            DiarizedSpan { start: 3.0, end: 4.0, cluster: 1 },
        ];
        // Cluster 0: 3 s dominant + 1 s not = 0.75 ≥ YOU_DOMINANCE.
        let d0 = cluster_mic_dominance(&windows, &spans, 0);
        assert!((d0 - 0.75).abs() < 1e-6, "got {d0}");
        assert!(d0 >= math::YOU_DOMINANCE);
        // Cluster 1 lives entirely in the loopback-dominant stretch.
        assert_eq!(cluster_mic_dominance(&windows, &spans, 1), 0.0);
        // No spans at all → no claim.
        assert_eq!(cluster_mic_dominance(&windows, &spans, 2), 0.0);
    }

    fn seg(start: f64, end: f64) -> TranscriptionSegment {
        TranscriptionSegment {
            speaker: None,
            speaker_id: None,
            text: "words".into(),
            start,
            end,
        }
    }

    #[test]
    #[ignore = "requires the speaker-id model and EMBRAL_TEST_DIARIZE_WAV"]
    fn speaker_pipeline_labels_and_suggests() {
        let wav = std::env::var("EMBRAL_TEST_DIARIZE_WAV").expect("set EMBRAL_TEST_DIARIZE_WAV");
        let samples = Arc::new(read_wav_16k(Path::new(&wav)).expect("read wav"));
        let engine = Engine::new();
        let db = Db::open_in_memory().unwrap();
        let config = AppConfig {
            speaker_match_mode: SpeakerMatchMode::Suggest,
            ..AppConfig::default()
        };

        // Segments roughly matching the synth turns (30 s file, alternating).
        let total = samples.len() as f64 / SAMPLE_RATE;
        let step = total / 6.0;
        let mut segments: Vec<TranscriptionSegment> = (0..6)
            .map(|i| seg(i as f64 * step + 0.2, (i + 1) as f64 * step - 0.2))
            .collect();

        // Pass 1 — empty registry: clusters get numbered labels, no suggestions.
        let input = PipelineInput {
            samples: samples.clone(),
            channel_windows: Vec::new(),
            meeting_id: "m1".into(),
        };
        let suggestions = run(&engine, &db, &config, &input, &mut segments).expect("pipeline");
        assert!(suggestions.is_empty());
        let labels: Vec<_> = segments.iter().filter_map(|s| s.speaker.clone()).collect();
        assert_eq!(labels.len(), 6, "every segment labeled");
        assert!(labels.contains(&"Speaker 1".to_string()));
        assert!(labels.contains(&"Speaker 2".to_string()));

        // Enroll speaker A (the first voice) from their own audio, then re-run:
        // their cluster should come back as a suggestion, the other must not.
        let spans = engine.diarize(&samples, 0.5).unwrap();
        let first_cluster = spans[0].cluster;
        let mut a_audio = Vec::new();
        for s in spans.iter().filter(|s| s.cluster == first_cluster) {
            let a = (s.start * SAMPLE_RATE) as usize;
            let b = ((s.end * SAMPLE_RATE) as usize).min(samples.len());
            a_audio.extend_from_slice(&samples[a..b]);
        }
        let embedding = engine.embed(&a_audio).unwrap();
        db.upsert_speaker(&SpeakerRow {
            id: "sp_david".into(),
            name: "David".into(),
            notes: String::new(),
            is_you: false,
        })
        .unwrap();
        db.add_voice_ref("sp_david", embral_db::VoiceRefKind::Enrolled, Some(1), &embedding, None, None)
            .unwrap();

        let mut segments2: Vec<TranscriptionSegment> = (0..6)
            .map(|i| seg(i as f64 * step + 0.2, (i + 1) as f64 * step - 0.2))
            .collect();
        let suggestions = run(&engine, &db, &config, &input, &mut segments2).expect("pipeline");
        eprintln!("suggestions: {suggestions:?}");
        assert_eq!(suggestions.len(), 1, "exactly the enrolled voice matches");
        assert_eq!(suggestions[0].name, "David");
        assert!(suggestions[0].score > 0.75);
        // The first segment belongs to David's cluster; its label must be the
        // one the suggestion points at.
        assert_eq!(segments2[0].speaker.as_deref(), Some(suggestions[0].label.as_str()));

        // Automatic mode assigns without asking and links the registry id.
        let auto_config = AppConfig {
            speaker_match_mode: SpeakerMatchMode::Automatic,
            ..AppConfig::default()
        };
        let mut segments3: Vec<TranscriptionSegment> = (0..6)
            .map(|i| seg(i as f64 * step + 0.2, (i + 1) as f64 * step - 0.2))
            .collect();
        let suggestions = run(&engine, &db, &auto_config, &input, &mut segments3).expect("pipeline");
        assert!(suggestions.is_empty());
        assert_eq!(segments3[0].speaker.as_deref(), Some("David"));
        assert_eq!(segments3[0].speaker_id.as_deref(), Some("sp_david"));
    }
}
