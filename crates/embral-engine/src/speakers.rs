//! Pure speaker-matching math — no ONNX, fully unit-testable.
//!
//! The diarization models (see [`crate::Engine::diarize`] / `embed`) produce
//! per-recording clusters and voice embeddings; everything that turns those
//! into decisions lives here: cosine scoring against a person's stored
//! reference set, the Off/Suggest/Automatic decision rule, mapping transcript
//! segments onto diarized spans, and the mic-dominance math behind "this
//! cluster is you".

use crate::engine::DiarizedSpan;

/// Minimum similarity for a cluster to be considered the same voice as a
/// stored reference set (see docs/phase4 §4.4).
pub const MATCH_THRESHOLD: f32 = 0.75;
/// Automatic mode stores the cluster embedding as a learned reference only at
/// this stricter similarity, so unattended matching can't drift the set.
pub const LEARN_THRESHOLD: f32 = 0.85;
/// Fraction of a cluster's (non-silent) speech time that must be
/// mic-dominant before the cluster is attributed to the user.
pub const YOU_DOMINANCE: f32 = 0.7;
/// The mic channel counts as dominant in a window when its RMS exceeds the
/// loopback RMS by this factor.
const MIC_DOMINANCE_RATIO: f32 = 2.0;
/// Windows quieter than this on both channels are silence and count neither
/// way (pauses inside a speech span must not dilute the fraction).
const RMS_FLOOR: f32 = 0.01;
/// Per-rank recency penalty when scoring a reference set (newest ref first).
/// Mild on purpose: recency mostly acts through pruning; this only breaks
/// near-ties toward how the person sounds lately.
const RECENCY_DECAY: f32 = 0.01;
const RECENCY_FLOOR: f32 = 0.9;

/// Cosine similarity in [-1, 1]; 0.0 for mismatched or empty inputs.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na * nb)
}

/// Element-wise mean of same-length embeddings; `None` when empty or ragged.
pub fn centroid(embeddings: &[Vec<f32>]) -> Option<Vec<f32>> {
    let dim = embeddings.first()?.len();
    if dim == 0 || embeddings.iter().any(|e| e.len() != dim) {
        return None;
    }
    let mut sum = vec![0.0f32; dim];
    for e in embeddings {
        for (s, v) in sum.iter_mut().zip(e) {
            *s += v;
        }
    }
    let n = embeddings.len() as f32;
    for s in &mut sum {
        *s /= n;
    }
    Some(sum)
}

/// Similarity of `candidate` to a person's reference set, ordered newest
/// first: the best single-reference cosine, mildly discounted by age rank.
pub fn score(candidate: &[f32], refs: &[Vec<f32>]) -> f32 {
    refs.iter()
        .enumerate()
        .map(|(rank, r)| {
            let w = (1.0 - RECENCY_DECAY * rank as f32).max(RECENCY_FLOOR);
            cosine(candidate, r) * w
        })
        .fold(0.0, f32::max)
}

/// Similarity floor for joining an existing live cluster during a recording.
/// Looser than [`MATCH_THRESHOLD`]: single-utterance embeddings are noisier
/// than the multi-span centroids registry matching sees, and a wrong live
/// label is a provisional preview the post-meeting pass overwrites, while a
/// spuriously split speaker is immediately visible noise.
pub const ONLINE_CLUSTER_THRESHOLD: f32 = 0.6;

/// One-pass greedy clustering over per-utterance voice embeddings — the live
/// counterpart of the recording-wide diarization pass. Each embedding joins
/// the most similar existing cluster at or above `threshold` (updating that
/// cluster's running-mean centroid) or opens a new one. Cluster indices are
/// 0-based in first-appearance order, matching the offline pipeline's
/// numbering convention.
pub struct OnlineClusterer {
    threshold: f32,
    centroids: Vec<Vec<f32>>,
    counts: Vec<f32>,
}

impl OnlineClusterer {
    pub fn new(threshold: f32) -> Self {
        Self {
            threshold,
            centroids: Vec::new(),
            counts: Vec::new(),
        }
    }

    /// Number of clusters seen so far.
    pub fn len(&self) -> usize {
        self.centroids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.centroids.is_empty()
    }

    /// Assign one embedding, returning its 0-based cluster index.
    pub fn assign(&mut self, embedding: &[f32]) -> usize {
        let best = self
            .centroids
            .iter()
            .enumerate()
            .map(|(i, c)| (i, cosine(c, embedding)))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        match best {
            Some((i, s)) if s >= self.threshold => {
                // Running mean; magnitude drift is irrelevant to cosine.
                let n = self.counts[i];
                for (c, e) in self.centroids[i].iter_mut().zip(embedding) {
                    *c = (*c * n + e) / (n + 1.0);
                }
                self.counts[i] += 1.0;
                i
            }
            _ => {
                self.centroids.push(embedding.to_vec());
                self.counts.push(1.0);
                self.centroids.len() - 1
            }
        }
    }
}

/// How matching behaves (mirrors the `speaker_match_mode` setting).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    Off,
    Suggest,
    Automatic,
}

/// The decision for one diarized cluster.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Assign this registry speaker without asking; `learn` says whether the
    /// cluster embedding is trustworthy enough to store as a learned ref.
    Auto { speaker_id: String, learn: bool },
    /// Surface a confirmation chip.
    Suggest { speaker_id: String, score: f32 },
    /// Nobody in the registry sounds like this cluster.
    Unknown,
}

/// Decide what to do with one cluster given its best-scoring registry
/// speaker. `best` is `(speaker_id, score)` from [`score`] over each person.
pub fn decide(best: Option<(&str, f32)>, mode: MatchMode) -> Outcome {
    let Some((speaker_id, s)) = best else {
        return Outcome::Unknown;
    };
    if mode == MatchMode::Off || s < MATCH_THRESHOLD {
        return Outcome::Unknown;
    }
    match mode {
        MatchMode::Automatic => Outcome::Auto {
            speaker_id: speaker_id.to_string(),
            learn: s >= LEARN_THRESHOLD,
        },
        MatchMode::Suggest => Outcome::Suggest {
            speaker_id: speaker_id.to_string(),
            score: s,
        },
        MatchMode::Off => unreachable!("handled above"),
    }
}

/// For each transcript segment `(start, end)`, the diarized cluster it
/// overlaps most, or `None` when it overlaps no span at all.
pub fn label_segments(segments: &[(f64, f64)], spans: &[DiarizedSpan]) -> Vec<Option<usize>> {
    segments
        .iter()
        .map(|&(start, end)| {
            let mut per_cluster: std::collections::HashMap<usize, f64> =
                std::collections::HashMap::new();
            for span in spans {
                let overlap = span.end.min(end) - span.start.max(start);
                if overlap > 0.0 {
                    *per_cluster.entry(span.cluster).or_default() += overlap;
                }
            }
            per_cluster
                .into_iter()
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(cluster, _)| cluster)
        })
        .collect()
}

/// One fixed-length stretch of recording time with the pre-mix loudness of
/// each capture channel. Produced by the recorder; timing is sample-counted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChannelWindow {
    pub start: f64,
    pub end: f64,
    pub mic_rms: f32,
    pub loop_rms: f32,
}

impl ChannelWindow {
    fn is_silent(&self) -> bool {
        self.mic_rms < RMS_FLOOR && self.loop_rms < RMS_FLOOR
    }
    fn mic_dominant(&self) -> bool {
        self.mic_rms >= RMS_FLOOR && self.mic_rms > self.loop_rms * MIC_DOMINANCE_RATIO
    }
}

/// Fraction of the non-silent window time inside `[start, end]` where the
/// mic drowned out the loopback channel. 0.0 when the range holds no
/// non-silent windows (an all-quiet span can't be claimed as "you").
pub fn mic_dominant_fraction(windows: &[ChannelWindow], start: f64, end: f64) -> f32 {
    let mut active = 0.0f64;
    let mut dominant = 0.0f64;
    for w in windows {
        let overlap = w.end.min(end) - w.start.max(start);
        if overlap <= 0.0 || w.is_silent() {
            continue;
        }
        active += overlap;
        if w.mic_dominant() {
            dominant += overlap;
        }
    }
    if active == 0.0 {
        0.0
    } else {
        (dominant / active) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start: f64, end: f64, cluster: usize) -> DiarizedSpan {
        DiarizedSpan { start, end, cluster }
    }

    #[test]
    fn cosine_basics() {
        assert_eq!(cosine(&[1.0, 0.0], &[1.0, 0.0]), 1.0);
        assert_eq!(cosine(&[1.0, 0.0], &[0.0, 1.0]), 0.0);
        assert_eq!(cosine(&[1.0, 0.0], &[-1.0, 0.0]), -1.0);
        // Scale-invariant.
        assert!((cosine(&[2.0, 2.0], &[5.0, 5.0]) - 1.0).abs() < 1e-6);
        // Degenerate inputs are 0, not NaN.
        assert_eq!(cosine(&[], &[]), 0.0);
        assert_eq!(cosine(&[1.0], &[1.0, 2.0]), 0.0);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
    }

    #[test]
    fn online_clusterer_groups_similar_voices_in_appearance_order() {
        let mut c = OnlineClusterer::new(0.6);
        // Two orthogonal "voices": every same-voice embedding rejoins its
        // cluster, and numbering follows first appearance.
        assert_eq!(c.assign(&[1.0, 0.0]), 0);
        assert_eq!(c.assign(&[0.9, 0.1]), 0);
        assert_eq!(c.assign(&[0.0, 1.0]), 1);
        assert_eq!(c.assign(&[0.1, 0.9]), 1);
        assert_eq!(c.assign(&[1.0, 0.05]), 0);
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn online_clusterer_centroid_tracks_the_running_mean() {
        let mut c = OnlineClusterer::new(0.6);
        c.assign(&[1.0, 0.0]);
        c.assign(&[0.0, 1.0]); // below threshold vs [1,0] → new cluster
        assert_eq!(c.len(), 2);
        // A vector between the two leans toward whichever centroid it joins;
        // after joining cluster 0 twice, the drifted centroid still owns it.
        assert_eq!(c.assign(&[0.8, 0.6]), 0);
        assert_eq!(c.assign(&[0.75, 0.65]), 0);
        // A pure second-axis vector still belongs to cluster 1.
        assert_eq!(c.assign(&[0.0, 1.0]), 1);
    }

    #[test]
    fn centroid_averages_and_rejects_ragged() {
        assert_eq!(
            centroid(&[vec![1.0, 0.0], vec![0.0, 1.0]]),
            Some(vec![0.5, 0.5])
        );
        assert_eq!(centroid(&[]), None);
        assert_eq!(centroid(&[vec![1.0], vec![1.0, 2.0]]), None);
    }

    #[test]
    fn score_takes_best_ref_with_mild_recency_discount() {
        let candidate = [1.0, 0.0];
        // Newest ref is orthogonal, an older one matches exactly: the old
        // match wins but pays a small rank discount.
        let refs = vec![vec![0.0, 1.0], vec![1.0, 0.0]];
        let s = score(&candidate, &refs);
        assert!(s > 0.98 && s < 1.0, "got {s}");
        // Same match as the newest ref scores exactly 1.0.
        assert_eq!(score(&candidate, &[vec![1.0, 0.0]]), 1.0);
        // Deep ranks never decay below the floor.
        let mut many = vec![vec![0.0, 1.0]; 30];
        many.push(vec![1.0, 0.0]);
        assert!(score(&candidate, &many) >= 0.9);
        assert_eq!(score(&candidate, &[]), 0.0);
    }

    #[test]
    fn decide_honors_mode_and_thresholds() {
        assert_eq!(decide(None, MatchMode::Automatic), Outcome::Unknown);
        assert_eq!(
            decide(Some(("sp", 0.9)), MatchMode::Off),
            Outcome::Unknown
        );
        assert_eq!(
            decide(Some(("sp", 0.74)), MatchMode::Suggest),
            Outcome::Unknown
        );
        assert_eq!(
            decide(Some(("sp", 0.8)), MatchMode::Suggest),
            Outcome::Suggest {
                speaker_id: "sp".into(),
                score: 0.8
            }
        );
        // Automatic assigns at the match threshold but only learns above the
        // stricter one.
        assert_eq!(
            decide(Some(("sp", 0.8)), MatchMode::Automatic),
            Outcome::Auto {
                speaker_id: "sp".into(),
                learn: false
            }
        );
        assert_eq!(
            decide(Some(("sp", 0.9)), MatchMode::Automatic),
            Outcome::Auto {
                speaker_id: "sp".into(),
                learn: true
            }
        );
    }

    #[test]
    fn label_segments_picks_max_overlap() {
        let spans = vec![span(0.0, 5.0, 0), span(5.0, 10.0, 1)];
        let labels = label_segments(
            &[
                (0.0, 4.0),  // inside cluster 0
                (4.0, 9.0),  // 1s of c0, 4s of c1
                (12.0, 13.0), // overlaps nothing
            ],
            &spans,
        );
        assert_eq!(labels, vec![Some(0), Some(1), None]);
    }

    #[test]
    fn label_segments_sums_split_spans_per_cluster() {
        // Cluster 0 speaks twice inside the segment; combined it out-overlaps
        // cluster 1's single longer turn.
        let spans = vec![span(0.0, 2.0, 0), span(3.0, 5.0, 0), span(2.0, 3.5, 1)];
        assert_eq!(label_segments(&[(0.0, 5.0)], &spans), vec![Some(0)]);
    }

    #[test]
    fn mic_dominance_fraction_ignores_silence() {
        let w = |start: f64, mic: f32, lb: f32| ChannelWindow {
            start,
            end: start + 0.5,
            mic_rms: mic,
            loop_rms: lb,
        };
        let windows = vec![
            w(0.0, 0.2, 0.01),   // mic dominant
            w(0.5, 0.2, 0.005),  // mic dominant
            w(1.0, 0.001, 0.001), // silence — excluded
            w(1.5, 0.05, 0.2),   // loopback dominant
        ];
        // 2 of 3 non-silent windows are mic-dominant.
        let f = mic_dominant_fraction(&windows, 0.0, 2.0);
        assert!((f - 2.0 / 3.0).abs() < 1e-6, "got {f}");
        // Range with only silence can't be claimed.
        assert_eq!(mic_dominant_fraction(&windows, 1.0, 1.5), 0.0);
        // Sub-window ranges weight by overlap.
        assert_eq!(mic_dominant_fraction(&windows, 0.0, 0.5), 1.0);
        // Near-equal channels (a shared mic picking up speakers) are not
        // dominant — the 2× ratio guards this.
        assert_eq!(
            mic_dominant_fraction(&[w(0.0, 0.1, 0.08)], 0.0, 0.5),
            0.0
        );
    }
}
