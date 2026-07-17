import { invoke } from '@tauri-apps/api/core';
import type { MeetingDetail, MeetingRecord, SegmentEdit } from '$lib/types';
import { ListSelection } from '$lib/utils/listSelection.svelte';

function isTauri() {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

/** Selection sentinel for the not-yet-persisted meeting finalizing in the
 * background (`appState.pendingMeeting`) — it has no database row yet. */
export const PENDING_MEETING_ID = '__pending__';

let _records = $state<MeetingRecord[]>([]);
let _details = $state<Record<string, MeetingDetail>>({});
let _selectedId = $state<string | null>(null);
let _isLoading = $state(false);
let _detailLoadingId = $state<string | null>(null);
let _error = $state<string | null>(null);

/** Multi-selection for the list. `selectedId` stays the *primary* row — the one
 * the detail pane shows — so everything that opens a single meeting (the
 * palette, the pending sentinel, a fresh load) is untouched by it. */
const _selection = new ListSelection();

async function loadDetail(id: string) {
  if (!isTauri() || _details[id]) return;
  _detailLoadingId = id;
  _error = null;
  try {
    const detail = await invoke<MeetingDetail>('get_meeting_detail', { id });
    _details = { ..._details, [id]: detail };
  } catch (e) {
    _error = e instanceof Error ? e.message : String(e);
  } finally {
    _detailLoadingId = null;
  }
}

async function load(limit = 100) {
  if (!isTauri()) return;
  _isLoading = true;
  _error = null;
  try {
    _records = await invoke<MeetingRecord[]>('get_meeting_records', {
      limit,
      since: null
    });
    // Rows can vanish under a selection (a delete elsewhere, a janitor prune).
    _selection.retain(_records.map((record) => record.id));
    // The pending sentinel is a valid selection even though no row backs it.
    if (
      !_selectedId ||
      (_selectedId !== PENDING_MEETING_ID &&
        !_records.some((record) => record.id === _selectedId))
    ) {
      const next = _records[0]?.id ?? null;
      _selectedId = next;
      if (next) _selection.select(next);
    }
    if (_selectedId && _selectedId !== PENDING_MEETING_ID) {
      await loadDetail(_selectedId);
    }
  } catch (e) {
    _error = e instanceof Error ? e.message : String(e);
  } finally {
    _isLoading = false;
  }
}

async function select(id: string) {
  _selectedId = id;
  _selection.select(id);
  if (id !== PENDING_MEETING_ID) {
    await loadDetail(id);
  }
}

/** A click in the list: plain, Ctrl (add/remove) or Shift (range). `order` is
 * the rows as they appear on screen, so a range crosses date headers. */
async function clickRow(
  id: string,
  event: { shiftKey: boolean; ctrlKey: boolean; metaKey: boolean },
  order: string[]
) {
  _selection.click(id, event, order);
  _selectedId = _selection.primary;
  if (_selectedId && _selectedId !== PENDING_MEETING_ID) {
    await loadDetail(_selectedId);
  }
}

/** Delete every selected meeting. The pending sentinel is skipped — it has no
 * row to delete, and it is about to become one. */
async function deleteSelected() {
  const ids = _selection.ids.filter((id) => id !== PENDING_MEETING_ID);
  if (ids.length === 0) return;
  _error = null;
  try {
    await invoke('delete_meetings', { ids });
    _records = _records.filter((record) => !ids.includes(record.id));
    const details = { ..._details };
    for (const id of ids) delete details[id];
    _details = details;

    _selection.retain(_records.map((record) => record.id));
    if (_selection.count === 0) {
      const next = _records[0]?.id ?? null;
      if (next) _selection.select(next);
      _selectedId = next;
    } else {
      _selectedId = _selection.primary;
    }
    if (_selectedId && _selectedId !== PENDING_MEETING_ID) await loadDetail(_selectedId);
  } catch (e) {
    _error = e instanceof Error ? e.message : String(e);
    throw e;
  }
}

function upsertDetail(detail: MeetingDetail) {
  _details = { ..._details, [detail.record.id]: detail };
  _records = _records
    .map((record) => (record.id === detail.record.id ? detail.record : record))
    .sort((a, b) => new Date(b.date).getTime() - new Date(a.date).getTime());
}

export const meetingsStore = {
  get records() {
    return _records;
  },
  get selectedId() {
    return _selectedId;
  },
  get selectedRecord() {
    return _records.find((record) => record.id === _selectedId) ?? null;
  },
  get selectedDetail() {
    return _selectedId ? (_details[_selectedId] ?? null) : null;
  },
  get isLoading() {
    return _isLoading;
  },
  get detailLoadingId() {
    return _detailLoadingId;
  },
  get error() {
    return _error;
  },
  /** The multi-selection. `selectedId` is its primary — what the detail shows. */
  get selection() {
    return _selection;
  },

  load,
  clickRow,
  deleteSelected,

  async refreshAndSelect(id?: string) {
    await load();
    const nextId = id ?? _records[0]?.id ?? null;
    if (nextId) {
      await select(nextId);
    }
  },

  select,

  loadDetail,

  async updateTitle(id: string, title: string) {
    if (!isTauri()) return;
    _error = null;
    try {
      const detail = await invoke<MeetingDetail>('update_meeting_title', { id, title });
      upsertDetail(detail);
      return detail;
    } catch (e) {
      _error = e instanceof Error ? e.message : String(e);
      throw e;
    }
  },

  async updateNotes(id: string, markdown: string) {
    if (!isTauri()) return;
    _error = null;
    try {
      const detail = await invoke<MeetingDetail>('update_meeting_notes', {
        id,
        markdown
      });
      upsertDetail(detail);
      return detail;
    } catch (e) {
      _error = e instanceof Error ? e.message : String(e);
      throw e;
    }
  },

  async updateTranscript(id: string, markdown: string) {
    if (!isTauri()) return;
    _error = null;
    try {
      const detail = await invoke<MeetingDetail>('update_meeting_transcript', {
        id,
        markdown
      });
      upsertDetail(detail);
      return detail;
    } catch (e) {
      _error = e instanceof Error ? e.message : String(e);
      throw e;
    }
  },

  async editSegments(id: string, edit: SegmentEdit) {
    if (!isTauri()) return;
    _error = null;
    try {
      const detail = await invoke<MeetingDetail>('edit_segments', {
        meetingId: id,
        edit
      });
      upsertDetail(detail);
      return detail;
    } catch (e) {
      _error = e instanceof Error ? e.message : String(e);
      throw e;
    }
  },

  async confirmSuggestion(id: string, label: string, speakerId: string) {
    if (!isTauri()) return;
    _error = null;
    try {
      const detail = await invoke<MeetingDetail>('confirm_speaker_suggestion', {
        meetingId: id,
        label,
        speakerId
      });
      upsertDetail(detail);
      return detail;
    } catch (e) {
      _error = e instanceof Error ? e.message : String(e);
      throw e;
    }
  },

  async dismissSuggestion(id: string, label: string, speakerId: string) {
    if (!isTauri()) return;
    _error = null;
    try {
      const detail = await invoke<MeetingDetail>('dismiss_speaker_suggestion', {
        meetingId: id,
        label,
        speakerId
      });
      upsertDetail(detail);
      return detail;
    } catch (e) {
      _error = e instanceof Error ? e.message : String(e);
      throw e;
    }
  },

  async deleteMeeting(id: string) {
    if (!isTauri()) return;
    _error = null;
    try {
      await invoke('delete_meeting', { id });
      _records = _records.filter((record) => record.id !== id);
      const { [id]: _removed, ...remaining } = _details;
      _details = remaining;
      if (_selectedId === id) {
        _selectedId = _records[0]?.id ?? null;
        if (_selectedId) {
          await loadDetail(_selectedId);
        }
      }
    } catch (e) {
      _error = e instanceof Error ? e.message : String(e);
      throw e;
    }
  }
};
