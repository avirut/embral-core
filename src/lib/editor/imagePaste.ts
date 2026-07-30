import { Extension } from '@tiptap/core';
import { Plugin, PluginKey } from '@tiptap/pm/state';
import { invoke } from '@tauri-apps/api/core';

/** Pasting an image into a document.
 *
 * The bytes go to the backend first and the node gets its real `src` when
 * that returns, so what the editor serializes is always a link to a file
 * that exists. While the save is in flight a placeholder node holds the
 * spot, keyed by a token so an undo (or a second paste landing first) can
 * never make us overwrite the wrong node.
 *
 * Mirrors `starGutter.ts`: the machinery lives here, not in the component. */

const key = new PluginKey('embral-image-paste');

/** Where the bytes go. `meetingId` absent = the recording happening now,
 * which the backend resolves from the recovery scratch. */
async function saveAsset(bytes: ArrayBuffer, meetingId?: string): Promise<string> {
  return invoke<string>('save_note_asset', bytes, {
    headers: meetingId ? { 'x-meeting-id': meetingId } : {}
  });
}

let nextToken = 1;

export interface ImagePasteOptions {
  /** The meeting this document belongs to; omit while recording. */
  meetingId?: () => string | undefined;
  /** Told when a paste fails, so the surface can say so in its own voice. */
  onError?: (message: string) => void;
}

export const imagePaste = (options: ImagePasteOptions = {}) =>
  Extension.create({
    name: 'embralImagePaste',
    addProseMirrorPlugins() {
      const editor = this.editor;
      return [
        new Plugin({
          key,
          props: {
            handlePaste(view, event) {
              const files = imageFilesFrom(event.clipboardData);
              if (files.length === 0) return false;
              // Take it: a screenshot paste usually carries text/plain
              // alongside, and letting that through would drop a file name
              // into the document next to the image.
              event.preventDefault();
              for (const file of files) void insert(file);
              return true;
            }
          }
        })
      ];

      async function insert(file: File) {
        const token = `pending-${nextToken++}`;
        // A placeholder with no src renders as a broken image, which is
        // honest: something is arriving and it is not here yet.
        editor
          .chain()
          .focus()
          .insertContent({ type: 'image', attrs: { src: '', title: token } })
          .run();

        try {
          const link = await saveAsset(await file.arrayBuffer(), options.meetingId?.());
          replacePlaceholder(token, link);
        } catch (e) {
          replacePlaceholder(token, null);
          options.onError?.(e instanceof Error ? e.message : String(e));
        }
      }

      /** Swap the placeholder for the real link, or remove it on failure.
       * Finding it by token rather than by position is what makes an undo
       * mid-flight harmless: the node is simply not there and nothing
       * happens. */
      function replacePlaceholder(token: string, link: string | null) {
        const { state } = editor.view;
        let found: number | null = null;
        state.doc.descendants((node, pos) => {
          if (found === null && node.type.name === 'image' && node.attrs.title === token) {
            found = pos;
          }
        });
        if (found === null) return;
        const tr = state.tr;
        if (link === null) {
          tr.delete(found, found + 1);
        } else {
          tr.setNodeMarkup(found, undefined, { src: link, title: null, alt: null });
        }
        editor.view.dispatch(tr);
      }
    }
  });

function imageFilesFrom(data: DataTransfer | null): File[] {
  if (!data) return [];
  return Array.from(data.files).filter((f) => f.type.startsWith('image/'));
}
