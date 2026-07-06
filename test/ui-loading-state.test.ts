import { describe, expect, test } from "bun:test";
import { planSessionLoad, shouldShowSessionSkeleton } from "../src/ui/loading-state.ts";

describe("session loading state", () => {
  test("refreshes the first page without depending on currently rendered rows", () => {
    expect(
      planSessionLoad({
        loadedRequestKey: "all:1",
        loadedRows: 100,
        pageSize: 100,
        requestKey: "all:2",
        sessionLimit: 100,
      }),
    ).toEqual({ limit: 100, offset: 0, replace: true });
  });

  test("preserves expanded load depth when refreshing in the background", () => {
    expect(
      planSessionLoad({
        loadedRequestKey: "all:1",
        loadedRows: 300,
        pageSize: 100,
        requestKey: "all:2",
        sessionLimit: 300,
      }),
    ).toEqual({ limit: 300, offset: 0, replace: true });
  });

  test("loads the next page only after the active request key is current", () => {
    expect(
      planSessionLoad({
        loadedRequestKey: "all:2",
        loadedRows: 100,
        pageSize: 100,
        requestKey: "all:2",
        sessionLimit: 200,
      }),
    ).toEqual({ limit: 100, offset: 100, replace: false });
  });

  test("does not fake a loading skeleton when rows are already visible", () => {
    expect(shouldShowSessionSkeleton({ isLoading: true, loadedRows: 100, query: "" })).toBe(false);
    expect(shouldShowSessionSkeleton({ isLoading: true, loadedRows: 0, query: "codex" })).toBe(
      false,
    );
    expect(shouldShowSessionSkeleton({ isLoading: true, loadedRows: 0, query: "" })).toBe(true);
  });
});
