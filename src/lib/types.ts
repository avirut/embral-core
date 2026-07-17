export interface TranscriptionSegment {
  speaker: string | null;
  // Registry link when this segment's speaker is a known person.
  speaker_id?: string | null;
  text: string;
  start: number;
  end: number;
}

/// Live in-flight utterance. `text` is the stable committed portion, while
/// `tentative_text` (when present) is an unstable trailing hypothesis that
/// may change on the next update — render it with reduced emphasis.
export interface InterimSegment {
  speaker: string | null;
  text: string;
  start: number;
  end: number;
  tentative_text: string | null;
}

export interface ProviderCapabilities {
  /** Provider labels are final; false = live labels are a provisional
   * preview the post-meeting pipeline overwrites. */
  labels_authoritative: boolean;
  max_session_minutes: number;
}

export interface MeetingRecord {
  id: string;
  title: string;
  date: string;
  duration_seconds: number;
  chunks: number;
  notes_path: string;
  transcript_path: string;
  audio_path: string;
}

export interface MeetingDetail {
  record: MeetingRecord;
  notes_markdown: string;
  transcript_markdown: string;
  audio_path: string | null;
  audio_exists: boolean;
  attendees: string[];
  // Structured transcript; empty for legacy meetings (raw editor fallback).
  segments: TranscriptionSegment[];
  speaker_suggestions: SpeakerSuggestion[];
  /** User-starred moments. */
  stars: MeetingStar[];
  /** The user's raw live notes, verbatim (the Notes tab). */
  user_notes: string;
}

/** One starred moment: when, and (when known) which top-level block of the
 * user's notes it sits on. */
export interface MeetingStar {
  seconds: number;
  note_block: number | null;
}

// A pending "Speaker N sounds like X" match awaiting confirmation.
export interface SpeakerSuggestion {
  label: string;
  speaker_id: string;
  name: string;
  score: number;
}

// One enrolled voice-reference slot on the Speakers page.
export interface VoiceSlot {
  slot: number;
  ref_id: number | null;
  clip_path: string | null;
}

// A registry person (mirrors src-tauri SpeakerProfile).
export interface SpeakerProfile {
  id: string;
  name: string;
  notes: string;
  is_you: boolean;
  voice_slots: VoiceSlot[];
  learned_refs: number;
  created_at: string;
  /** The newest meeting they were in; null if never seen in one. The profiles
   * list sorts and groups on this, falling back to created_at. */
  last_seen: string | null;
}

// One transcript edit operation for edit_segments.
export type SegmentEdit =
  | { kind: 'split'; index: number; char_offset: number }
  | { kind: 'delete'; index: number }
  | { kind: 'reassign'; index: number; speaker: string; speaker_id?: string | null }
  | { kind: 'relabel_all'; from: string; to: string; speaker_id?: string | null }
  | { kind: 'clear_label'; label: string };

export interface MeetingSummary {
  id: string;
  title: string;
  date: string;
  duration_seconds: number;
}

// 'cloud' exists only in cloud-edition builds (embral's metered backend).
export type TranscriptionProvider = 'local' | 'cloud';

// The language transcription runs in — owned above the provider choice and
// read by both. 'multilingual' means "detect it as it is spoken".
export type TranscriptionLanguage = 'english' | 'multilingual';

// What a cloud recording does when the account's hours — subscription plus
// purchased — run out. 'disabled' keeps recording and note-taking but writes
// no transcript. A dropped connection always lands on the device regardless.
export type CloudOutOfHours = 'local' | 'disabled';

// One local speech model from the engine catalog (mirrors
// embral-engine::ModelStatus).
export interface ModelStatus {
  id: string;
  display_name: string;
  kind: 'streaming_asr' | 'offline_asr' | 'punctuation' | 'speaker_id' | 'llm' | 'embedding';
  note: string;
  // ISO codes, or ['*'] for language-independent models.
  languages: string[];
  present: boolean;
  total_bytes: number;
  dir: string;
  // Vocabulary boost availability (sherpa runtime limitation per model).
  supports_hotwords: boolean;
  // True when the model punctuates/cases natively (no punct model needed).
  native_punctuation: boolean;
}

// Byte-level progress for one model download (model-download-progress event).
export interface ModelProgress {
  model_id: string;
  file_name: string;
  downloaded_bytes: number;
  total_bytes: number;
}

// The palette's hybrid search results (search_library) — meetings grouped
// best-passage-per-meeting, dictations alongside. Snippets carry [match]
// markers when the keyword leg produced them.
export interface LibraryMeetingHit {
  id: string;
  title: string;
  started_at: string;
  snippet: string;
}
export interface LibraryDictationHit {
  id: number;
  snippet: string;
  created_at: string;
}
export interface LibrarySearchResults {
  meetings: LibraryMeetingHit[];
  dictations: LibraryDictationHit[];
}

export type Theme = 'system' | 'light' | 'dark';
export type AutoStartPolicy = 'always' | 'selective' | 'prompt' | 'manual';
export type SpeakerMatchMode = 'off' | 'suggest' | 'automatic';
export type DiarizationSensitivity = 'low' | 'medium' | 'high';
export type LlmProvider = 'builtin' | 'custom';
// The three documents a meeting carries. A meeting with no summary opens on
// notes whatever this says (its Summary tab doesn't exist).
export type OpenMeetingTab = 'summary' | 'notes' | 'transcript';
// 'cloud' exists only in cloud-edition builds. Cloud degrades to on-device
// while signed out; any failure delivers the raw text.
export type DictationCleanup = 'cloud' | 'on_device' | 'off';

// One synthesis engine (mirrors embral-types::LlmProfile). The list is
// fixed per edition — see utils/llmProfiles.ts availableProfiles().
export interface LlmProfile {
  id: string;
  name: string;
  provider: LlmProvider;
  model: string;
  endpoint: string;
  api_key: string;
}

export const BUILTIN_PROFILE_ID = 'builtin';

// One saved dictation (mirrors embral-db::DictationRow).
export interface DictationRow {
  id: number;
  raw_text: string;
  cleaned_text: string | null;
  app: string | null;
  created_at: string;
}
export type ExportMetadataFormat = 'frontmatter' | 'inline';
export type WebhookMethod = 'post' | 'put';

// Device names reported by list_audio_devices.
export interface AudioDevices {
  inputs: string[];
  outputs: string[];
}

export interface AppConfig {
  transcription_provider: TranscriptionProvider;
  transcription_language: TranscriptionLanguage;
  // Cloud edition only — absent from the offline build's config.
  cloud_out_of_hours?: CloudOutOfHours;
  // Cloud edition only: this device's session token; empty = signed out.
  // Read by the local-LLM usage rule, never written frontend-side.
  cloud_session_token?: string;
  storage_dir: string;
  retain_audio: boolean;
  // Local (on-device) transcription
  local_asr_model: string;
  vocabulary: string[];
  // Post-meeting integrations
  obsidian_export_enabled: boolean;
  obsidian_vault_dir: string;
  webhook_url: string;
  webhook_method: WebhookMethod;
  export_filename_template: string;
  export_metadata_format: ExportMetadataFormat;
  // Appearance & app behavior
  theme: Theme;
  mic_device: string;
  output_device: string;
  notify_summary_ready: boolean;
  notify_recording_started: boolean;
  notify_update_available: boolean;
  audio_retention_days: number;
  meeting_retention_days: number;
  onboarding_completed: boolean;
  // Meeting detection & automation
  auto_start_policy: AutoStartPolicy;
  auto_detect_apps: string[];
  detection_delay_secs: number;
  auto_stop_enabled: boolean;
  notify_call_detected: boolean;
  record_hotkey: string;
  sidebar_expanded: boolean;
  // Speakers
  diarization_enabled: boolean;
  diarization_sensitivity: DiarizationSensitivity;
  speaker_match_mode: SpeakerMatchMode;
  // Synthesis
  summaries_enabled: boolean;
  // The engine: "builtin", or "cloud" in cloud builds. Only consulted while
  // summaries_enabled.
  summaries_profile_id: string;
  // Full replacement prompt body; "" = built-in default. The locked output
  // contract is appended backend-side either way.
  summary_prompt: string;
  open_meeting_tab: OpenMeetingTab;
  llm_keep_warm: boolean;
  llm_idle_minutes: number;
  // Dictation — its own transcription tree, independent of meetings.
  dictation_hotkey: string;
  dictation_provider: TranscriptionProvider;
  // Cloud edition only — absent from the offline build's config.
  dictation_out_of_hours?: CloudOutOfHours;
  dictation_language: TranscriptionLanguage;
  dictation_asr_model: string;
  dictation_cleanup: DictationCleanup;
  dictation_copy_clipboard: boolean;
  dictation_auto_paste: boolean;
  dictation_auto_delete: boolean;
  dictation_retention_days: number;
  dictation_retention_count: number;
}
