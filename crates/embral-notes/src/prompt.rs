//! The refinement system prompt and user-message builder.
//!
//! Extracted from `src-tauri/src/refinement.rs` so it can be unit-tested and
//! shared across every notes engine unchanged — the prompt is engine-neutral
//! (it targets a generic chat model); only the transport differs.

/// The editable half of the summary prompt. Users may replace this wholesale
/// in Settings; it deliberately contains no output-format rules — those live
/// in [`OUTPUT_CONTRACT`], which is always appended so the machine-read
/// pieces (frontmatter, Chapters) survive any customization.
pub const DEFAULT_SUMMARY_PROMPT: &str = r#"You are a professional meeting notes assistant. Transform the raw diarized transcript and user notes into a clean, structured markdown document.

Core principles:
- You are restructuring, not summarizing. Write for a smart, busy reader who has context.
- Cut connective tissue and narration. "John opened by framing the meeting around several priorities" becomes "John identified three priorities: A, B, and C."
- Inline quotes directly rather than attributing them with extra words. "Sarah expressed frustration — 'I wish Bob had a Lisa'" not "Sarah's exact phrase was 'I wish Bob had a Lisa.'"
- Preserve specific details: names, numbers, examples, decisions, direct quotes. Strip the scaffolding around them.
- Do not editorialize or add interpretation that wasn't in the source material.
- Attribute statements when the identity of the speaker matters (decision-maker, subject expert). Skip attribution for general group discussion.
- Do not narrate transitions between topics. Just present the content.

Treating user notes:
- The user message contains a USER NOTES section authored live by the meeting participant. These represent what the user found important enough to write down themselves — treat them as the highest-signal input.
- Every distinct point in user notes must be reflected somewhere in your output: usually surfaced in Key Takeaways, Next Steps, or the relevant segment. If user notes name a decision, action, or owner, prefer that framing over what the transcript implies.
- If user notes contradict the transcript on a fact, prefer the user notes (the user heard the room better than the transcript can capture).
- Do not echo user notes verbatim as a standalone section — the app preserves them separately. Your job is to integrate their content into the structured notes.

Identifying attendees:
- Populate the frontmatter attendees list from the full source material, especially USER NOTES when they mention real names.
- Prefer real attendee names over generic diarization labels like "Speaker 1".
- Use transcript content to infer names when people introduce themselves, address each other, assign owners, or refer to participants.
- Only fall back to generic speaker labels when no real names can be inferred.
- Do not include both a real name and its generic speaker label unless they clearly refer to different people."#;

/// The locked half: the exact output shape the app parses (frontmatter fields,
/// the Chapters format, the section scaffold). Appended after the editable
/// prompt unconditionally.
pub const OUTPUT_CONTRACT: &str = r#"Output format (follow exactly):

---
start_time: [ISO 8601 timestamp, e.g. 2026-03-26T14:30:00Z]
duration_minutes: [integer, rounded]
meeting_id: [provided in the user message]
attendees: ["list of names if identifiable", "otherwise speaker labels used"]
---

# [Meeting Title — 5-8 words, descriptive, auto-generated from content]

## Key Takeaways
[3-4 bullets, each under 30 words. Highest-signal conclusions or decisions, not a recap of every topic.]

## Next Steps
[Tactical action items. Format: **[Owner]** Action in ~15 words. Sub-bullets for detail if needed. Every item tied to a person.]

## [Segment 1 Title]
[Concise, skimmable notes. Use bullets for discrete points, short prose for flow. Every sentence must carry information.]

## [Segment 2 Title]
[...]

Segment titles should be descriptive but concise (e.g., "Product Roadmap Discussion" not "Discussion About Various Product Roadmap Items and Related Topics"). Aim for 3-6 segments.

Note on speaker labels: The transcript uses generic speaker labels. If no labels are present, the provider does not support diarization — work with the plain text. If you can identify speakers by name from what they discuss, use their names."#;

/// The effective system prompt: the user's custom prompt when set (else the
/// default), always followed by the locked output contract.
pub fn system_prompt(custom_prompt: &str) -> String {
    let body = custom_prompt.trim();
    let body = if body.is_empty() { DEFAULT_SUMMARY_PROMPT } else { body };
    format!("{body}\n\n{OUTPUT_CONTRACT}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_prompt_gets_the_contract_appended() {
        let p = system_prompt("");
        assert!(p.starts_with(DEFAULT_SUMMARY_PROMPT));
        assert!(p.ends_with(OUTPUT_CONTRACT));
        // Whitespace-only counts as unset.
        assert_eq!(system_prompt("   \n"), p);
    }

    #[test]
    fn custom_prompt_replaces_the_body_but_never_the_contract() {
        let p = system_prompt("Write everything in pirate speak.");
        assert!(p.starts_with("Write everything in pirate speak."));
        assert!(!p.contains("professional meeting notes assistant"));
        assert!(p.contains("## Key Takeaways"), "contract survives customization");
        assert!(p.contains("start_time:"), "frontmatter contract survives");
    }

    #[test]
    fn contract_holds_the_machine_read_formats() {
        // The pieces frontmatter parsing relies on must live in the locked
        // half, not the editable default.
        assert!(OUTPUT_CONTRACT.contains("attendees:"));
        assert!(OUTPUT_CONTRACT.contains("## Key Takeaways"));
        assert!(!DEFAULT_SUMMARY_PROMPT.contains("start_time:"));
    }
}

/// System prompt for dictation cleanup. One prompt covers both modes: silent
/// cleanup by default, instruction-following when the dictation itself opens
/// with a command.
pub const DICTATION_SYSTEM_PROMPT: &str = r#"You process raw speech-to-text dictation. Reply with the processed text only — no preamble, no explanations, no quotes around the result, no markdown code fences.

Default behavior — clean up:
- Fix punctuation, capitalization, and obvious transcription errors.
- Remove filler words (um, uh, you know, like) and false starts ("I want— I need the report").
- Apply spoken formatting commands: "new line", "new paragraph", "comma", "period" become the actual formatting when clearly meant as commands.
- Keep the person's words, tone, and meaning. Do not summarize, shorten, embellish, or add content. Keep the language the dictation was spoken in.

Instruction mode — only when the dictation clearly OPENS with an imperative directed at you (e.g. "make a bulleted grocery list: milk eggs bread", "write a polite reply saying I can't make it"): execute that instruction on the rest of the dictation and reply with the requested text instead of a cleaned transcript.

If in doubt whether something is an instruction, treat it as content and just clean it up."#;

/// Build the dictation user message.
pub fn build_dictation_message(raw: &str) -> String {
    format!("DICTATION:\n{}", raw.trim())
}

/// Build the user message packing meeting metadata, an optional user-supplied
/// title directive, the live user-notes block, and the transcript. Identical
/// shape to the original `refine_notes` inline construction so provider output
/// is unchanged.
pub fn build_user_message(
    meeting_id: &str,
    start_time: &str,
    duration_minutes: u32,
    meeting_title: Option<&str>,
    transcript: &str,
    user_notes: Option<&str>,
) -> String {
    let user_notes_block = match user_notes.map(str::trim).filter(|s| !s.is_empty()) {
        Some(notes) => format!(
            "USER NOTES (human-authored, high-signal — these are the user's own emphasis, decisions, and reminders captured live; treat as authoritative for what mattered):\n{}",
            notes
        ),
        None => "USER NOTES: (none provided)".to_string(),
    };

    let title_block = match meeting_title.map(str::trim).filter(|s| !s.is_empty()) {
        Some(title) => format!(
            "USER-PROVIDED MEETING TITLE: {}\nUse this exact title as the level-one markdown heading. Do not invent a different meeting title.",
            title
        ),
        None => {
            "USER-PROVIDED MEETING TITLE: (none provided; generate a descriptive title)".to_string()
        }
    };

    format!(
        "MEETING_ID: {}\nSTART_TIME: {}\nDURATION_MINUTES: {}\n{}\n\n{}\n\nTRANSCRIPT:\n{}",
        meeting_id, start_time, duration_minutes, title_block, user_notes_block, transcript,
    )
}
