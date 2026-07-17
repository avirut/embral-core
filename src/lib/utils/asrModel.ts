import type { AppConfig } from '$lib/types';

/** The one catalog model covering languages beyond English. The accuracy tier
 * is an English concept — there is nothing to choose between here. Mirrors
 * `embral_types::MULTILINGUAL_ASR_MODEL`. */
export const MULTILINGUAL_ASR_MODEL = 'parakeet-tdt-v3';

/** The model on-device transcription actually runs. `local_asr_model` holds the
 * *English* accuracy choice, so another language overrides it rather than
 * overwriting it — switching back restores the tier the user picked. Mirrors
 * `AppConfig::meeting_asr_model` (keep the two in step). */
export function meetingAsrModel(config: AppConfig): string {
  return config.transcription_language === 'multilingual'
    ? MULTILINGUAL_ASR_MODEL
    : config.local_asr_model;
}

/** The model on-device dictation runs, governed by dictation's *own*
 * language; an empty setting follows the meeting model. Mirrors
 * `AppConfig::dictation_asr_model_id`. */
export function dictationAsrModel(config: AppConfig): string {
  if (config.dictation_language === 'multilingual') return MULTILINGUAL_ASR_MODEL;
  return config.dictation_asr_model.trim() || config.local_asr_model;
}
