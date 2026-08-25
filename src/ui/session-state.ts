export type SessionStateUpdate = "archived" | "deleted" | "visible";

export interface SessionArchiveView {
  user_state: "archived" | null;
  is_user_archived: boolean;
}

/**
 * Sits directly above DELETE_SESSION_EXPLANATION. Deletion is irreversible --
 * tombstones stop any later sync from re-ingesting the source -- but it does
 * not erase the bytes, which survive until `decant db vacuum`. The header
 * claims the first and not the second.
 */
export const DELETE_SESSION_EYEBROW = "Cannot be undone";

export const DELETE_SESSION_EXPLANATION =
  "This removes this session and its subagent transcripts from the Decant archive. " +
  "The source JSONL files on disk are not changed. A deletion tombstone prevents future syncs " +
  "from restoring these sessions. SQLite frees the deleted rows without overwriting them, so " +
  "their text stays readable inside the archive file until you run `decant db vacuum`.";

export function archiveActionFor(
  session: SessionArchiveView,
): Extract<SessionStateUpdate, "archived" | "visible"> | null {
  if (session.user_state === "archived") {
    return "visible";
  }
  return session.is_user_archived ? null : "archived";
}

export function sessionStateRequest(
  id: number,
  state: SessionStateUpdate,
): { path: string; init: RequestInit } {
  return {
    path: `/api/sessions/${id}/state`,
    init: {
      method: "POST",
      body: JSON.stringify({ state }),
    },
  };
}
