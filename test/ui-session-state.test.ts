import { describe, expect, test } from "bun:test";
import {
  archiveActionFor,
  DELETE_SESSION_EXPLANATION,
  DELETE_SESSION_EYEBROW,
  sessionStateRequest,
} from "../src/ui/session-state.ts";

describe("session state UI", () => {
  test("offers archive, direct unarchive, or no-op for inherited archive state", () => {
    expect(archiveActionFor({ user_state: null, is_user_archived: false })).toBe("archived");
    expect(archiveActionFor({ user_state: "archived", is_user_archived: true })).toBe("visible");
    expect(archiveActionFor({ user_state: null, is_user_archived: true })).toBeNull();
  });

  test("builds the local state mutation request", () => {
    expect(sessionStateRequest(42, "deleted")).toEqual({
      path: "/api/sessions/42/state",
      init: {
        method: "POST",
        body: '{"state":"deleted"}',
      },
    });
  });

  test("delete confirmation distinguishes archive data from source files and sync behavior", () => {
    expect(DELETE_SESSION_EXPLANATION).toContain("Decant archive");
    expect(DELETE_SESSION_EXPLANATION).toContain("source JSONL files on disk are not changed");
    expect(DELETE_SESSION_EXPLANATION).toContain("future syncs from restoring");
  });

  test("delete confirmation does not promise more than a row delete gives", () => {
    // SQLite frees the pages without zeroing them, so the transcript text stays
    // greppable in the archive file until a vacuum rewrites it. Copy that says
    // "permanently" without saying that is wrong where it matters most.
    expect(DELETE_SESSION_EXPLANATION).not.toContain("permanently");
    expect(DELETE_SESSION_EXPLANATION).toContain("decant db vacuum");
  });

  test("delete confirmation header says what the body says", () => {
    // The eyebrow sat directly above body copy that had "permanently" removed
    // from it. Deletion IS irreversible -- there is no un-delete -- but it does
    // not erase the bytes, so the header claims irreversibility, not erasure.
    expect(DELETE_SESSION_EYEBROW).not.toContain("Permanent");
    expect(DELETE_SESSION_EYEBROW).toBe("Cannot be undone");
  });
});
