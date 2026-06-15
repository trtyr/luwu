// hooks/usePasteHandler.ts — Paste detection + multi-chunk assembly + chip-ification
//
// Based on Claude Code doc 32 §2 (paste subsystem).
//
// Detection strategy (dual-signal):
//   Signal A (primary): bracketed paste markers (ESC[200~ ... ESC[201~)
//     - Terminal wraps pasted content when DEC mode 2004 is enabled
//     - Markers are detected via raw stdin prependListener (fires before Ink)
//     - 100% reliable — terminal explicitly says "this is a paste"
//   Signal B (fallback): input length > PASTE_THRESHOLD
//     - For terminals that don't support bracketed paste
//     - Rapid multi-char input is treated as paste
//
// Multi-chunk assembly:
//   Node stdin is not atomic — a 5000-char paste arrives as multiple chunks.
//   Bracketed paste: markers delimit boundaries (no timer needed).
//   Non-bracketed: 100ms timer assembles rapid successive inputs.
//
// Chip-ification:
//   Pastes > PASTE_THRESHOLD (800 chars) or > maxLines show as [↵ N lines]
//   reference instead of expanding into the input box (which would cause
//   layout explosion and trigger full-screen repaints).

import { useEffect, useRef, useCallback } from 'react';

const PASTE_START = '\x1b[200~';
const PASTE_END = '\x1b[201~';
const PASTE_THRESHOLD = 800;
const PASTE_TIMEOUT_MS = 100;
const MAX_PASTE_DISPLAY_LINES = 2;

export interface PasteResult {
  text: string;
  lineCount: number;
  isLarge: boolean;
}

export interface UsePasteHandlerReturn {
  /** True while inside a bracketed paste (ESC[200~ seen, ESC[201~ not yet) */
  isPasteActive: React.MutableRefObject<boolean>;
  /** True if recently completed a paste (resets after 50ms) */
  justPastedRef: React.MutableRefObject<boolean>;
  /**
   * Feed input through paste detection. Returns true if the input was
   * consumed by paste handling (caller should skip normal processing).
   */
  checkInput: (input: string) => boolean;
}

export function usePasteHandler(
  onPaste: (result: PasteResult) => void,
  enabled: boolean,
): UsePasteHandlerReturn {
  // ── Bracketed paste state ──
  const isInPasteRef = useRef(false);
  const justPastedRef = useRef(false);
  const pasteBufferRef = useRef('');
  const onPasteRef = useRef(onPaste);
  onPasteRef.current = onPaste;

  // ── Non-bracketed fallback state ──
  const fallbackBufferRef = useRef('');
  const fallbackTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // ── Flush paste buffer ──
  const flush = useCallback((raw: string) => {
    if (raw.length === 0) return;

    const cleaned = raw
      .replace(/\r\n?/g, '\n')
      .replaceAll('\t', '    ');

    const lineCount = cleaned.split('\n').length;
    const isLarge =
      cleaned.length > PASTE_THRESHOLD ||
      lineCount > MAX_PASTE_DISPLAY_LINES;

    onPasteRef.current({ text: cleaned, lineCount, isLarge });
  }, []);

  // ── Bracketed paste listener (prependListener = fires before Ink) ──
  useEffect(() => {
    if (!enabled) return;
    // Only enable in TTY (not in tests/pipes)
    if (!process.stdin.isTTY) return;

    const handler = (chunk: Buffer) => {
      let data = chunk.toString('utf8');

      while (data.length > 0) {
        if (isInPasteRef.current) {
          // Inside paste — looking for PASTE_END
          const endIdx = data.indexOf(PASTE_END);
          if (endIdx !== -1) {
            pasteBufferRef.current += data.slice(0, endIdx);
            data = data.slice(endIdx + PASTE_END.length);
            isInPasteRef.current = false;
            // Set justPasted to suppress Ink's useInput for ~50ms
            justPastedRef.current = true;
            setTimeout(() => { justPastedRef.current = false; }, 50);
            // Flush paste content
            flush(pasteBufferRef.current);
            pasteBufferRef.current = '';
          } else {
            // Still accumulating — no end marker yet
            pasteBufferRef.current += data;
            data = '';
          }
        } else {
          // Outside paste — looking for PASTE_START
          const startIdx = data.indexOf(PASTE_START);
          if (startIdx !== -1) {
            isInPasteRef.current = true;
            pasteBufferRef.current = '';
            data = data.slice(startIdx + PASTE_START.length);
          } else {
            // No paste marker — regular data, let it through
            data = '';
          }
        }
      }
    };

    process.stdin.prependListener('data', handler);
    return () => {
      process.stdin.removeListener('data', handler);
    };
  }, [enabled, flush]);

  // ── checkInput: called from useInput to detect non-bracketed paste ──
  const checkInput = useCallback((input: string): boolean => {
    // If bracketed paste handler already processed this, skip
    if (isInPasteRef.current || justPastedRef.current) {
      return true; // consume — don't process as normal input
    }

    // Non-bracketed paste detection: multi-char input with newlines or very long
    // Normal keyboard input is 1-2 chars per keystroke
    const looksLikePaste =
      input.length > PASTE_THRESHOLD ||
      (input.length > 3 && /[\n\r\t]/.test(input));

    if (!looksLikePaste) return false; // not paste — process normally

    // Start or continue fallback accumulation
    fallbackBufferRef.current += input;

    // Reset timer — if no new chunk in PASTE_TIMEOUT_MS, flush
    if (fallbackTimerRef.current) clearTimeout(fallbackTimerRef.current);
    fallbackTimerRef.current = setTimeout(() => {
      const raw = fallbackBufferRef.current;
      fallbackBufferRef.current = '';
      fallbackTimerRef.current = null;
      flush(raw);
    }, PASTE_TIMEOUT_MS);

    return true; // consumed by paste handler
  }, []);

  return {
    isPasteActive: isInPasteRef,
    justPastedRef,
    checkInput,
  };
}
